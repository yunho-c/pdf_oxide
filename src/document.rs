//! PDF document model.

use crate::encryption::EncryptionHandler;
use crate::error::{Error, Result};
use crate::layout::TextSpan;
use crate::object::{Object, ObjectRef};
use crate::parser::parse_object;
use crate::parser_config::ParserOptions;
use crate::pipeline::{
    converters::OutputConverter, HtmlOutputConverter, MarkdownOutputConverter, PlainTextConverter,
    ReadingOrderContext, TextPipeline, TextPipelineConfig,
};
use crate::structure::traverse_structure_tree;
use crate::xref::{find_xref_offset, parse_xref, CrossRefTable};
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// Maximum recursion depth for object resolution
const MAX_RECURSION_DEPTH: u32 = 100;

/// Page information for rendering.
#[cfg(feature = "rendering")]
#[derive(Debug, Clone)]
pub struct PageInfo {
    /// Media box defining the page boundaries
    pub media_box: crate::geometry::Rect,
    /// Crop box if specified (for visible area)
    pub crop_box: Option<crate::geometry::Rect>,
    /// Page rotation in degrees (0, 90, 180, 270)
    pub rotation: i32,
}

/// PDF document.
///
/// This structure represents an open PDF document, providing access to:
/// - Document metadata (version, catalog, trailer)
/// - Page information (count, page tree)
/// - Object loading and dereferencing
///
/// # Example
///
/// ```no_run
/// use pdf_oxide::document::PdfDocument;
///
/// let mut doc = PdfDocument::open("sample.pdf")?;
/// println!("PDF version: {}.{}", doc.version().0, doc.version().1);
/// println!("Page count: {}", doc.page_count()?);
/// # Ok::<(), pdf_oxide::error::Error>(())
/// ```
pub struct PdfDocument {
    /// Buffered reader for the PDF file
    reader: BufReader<File>,
    /// PDF version (major, minor)
    version: (u8, u8),
    /// Cross-reference table mapping object IDs to byte offsets
    xref: CrossRefTable,
    /// Trailer dictionary
    trailer: Object,
    /// Cache for loaded objects to avoid re-parsing
    object_cache: HashMap<ObjectRef, Object>,
    /// Track objects being resolved (for cycle detection)
    resolving_stack: RefCell<HashSet<ObjectRef>>,
    /// Current recursion depth
    recursion_depth: RefCell<u32>,
    /// Encryption handler (if PDF is encrypted)
    encryption_handler: Option<EncryptionHandler>,
    /// Parser configuration options for error handling and recovery
    #[allow(dead_code)]
    options: ParserOptions,
    /// Byte offset where PDF header was found (may not be 0 for malformed PDFs)
    #[allow(dead_code)]
    header_offset: u64,
}

impl std::fmt::Debug for PdfDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfDocument")
            .field("version", &self.version)
            .field("xref_entries", &self.xref.len())
            .field("cached_objects", &self.object_cache.len())
            .field("recursion_depth", &self.recursion_depth.borrow())
            .finish_non_exhaustive()
    }
}

impl PdfDocument {
    /// Open a PDF document from a file path.
    ///
    /// This function:
    /// 1. Opens the file
    /// 2. Parses the PDF header to validate and extract version
    /// 3. Locates and parses the cross-reference table
    /// 4. Parses the trailer dictionary
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened
    /// - The PDF header is invalid or unsupported
    /// - The cross-reference table cannot be found or parsed
    /// - The trailer dictionary is invalid
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pdf_oxide::document::PdfDocument;
    ///
    /// let doc = PdfDocument::open("sample.pdf")?;
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path.as_ref())?;
        let mut reader = BufReader::new(file);

        // Parse header with lenient mode by default (handle PDFs with binary prefixes)
        let (major, minor, header_offset) = parse_header(&mut reader, true)?;
        let version = (major, minor);

        // Try to parse xref table normally
        let (xref, trailer) = match Self::try_open_regular(&mut reader) {
            Ok((xref, trailer)) => {
                // Success with regular parsing
                // However, if the xref is suspiciously small (< 5 entries), it's likely corrupted
                // Try reconstruction to get a complete table
                if xref.is_empty() {
                    log::warn!(
                        "Regular xref parsing succeeded but table is empty, attempting reconstruction"
                    );
                    Self::try_reconstruct_xref(&mut reader)?
                } else if xref.len() < 5 {
                    log::warn!(
                        "Regular xref parsing succeeded but only found {} entries (suspiciously small), attempting reconstruction",
                        xref.len()
                    );
                    // Try reconstruction, but keep the original if reconstruction fails
                    match Self::try_reconstruct_xref(&mut reader) {
                        Ok((reconstructed_xref, reconstructed_trailer)) => {
                            log::info!(
                                "Reconstruction found {} entries (vs {} in damaged xref)",
                                reconstructed_xref.len(),
                                xref.len()
                            );
                            (reconstructed_xref, reconstructed_trailer)
                        },
                        Err(e) => {
                            log::warn!("Reconstruction failed: {}, using original damaged xref", e);
                            (xref, trailer)
                        },
                    }
                } else {
                    (xref, trailer)
                }
            },
            Err(e) => {
                log::warn!("Regular xref parsing failed: {}, attempting reconstruction", e);

                // Fall back to xref reconstruction
                match Self::try_reconstruct_xref(&mut reader) {
                    Ok((reconstructed_xref, reconstructed_trailer)) => {
                        log::info!("Successfully reconstructed xref table");
                        (reconstructed_xref, reconstructed_trailer)
                    },
                    Err(recon_err) => {
                        log::error!("XRef reconstruction also failed: {}", recon_err);
                        return Err(e); // Return original error
                    },
                }
            },
        };

        // Note: Encryption initialization was originally lazy, but decode_stream_with_encryption
        // only has &self access which prevents initialization.
        // We now initialize eagerly to ensure the handler is ready when needed.
        let mut document = Self {
            reader,
            version,
            xref,
            trailer,
            object_cache: HashMap::new(),
            resolving_stack: RefCell::new(HashSet::new()),
            recursion_depth: RefCell::new(0),
            encryption_handler: None,
            options: ParserOptions::default(),
            header_offset,
        };

        // Initialize encryption immediately
        if let Err(e) = document.ensure_encryption_initialized() {
            log::error!("Failed to initialize encryption: {}", e);
            // We continue anyway, as it might just be an unsupported security handler
            // and maybe we can still read parts of the file (or fail later)
        }

        Ok(document)
    }

    /// Try to open the PDF using regular xref parsing.
    fn try_open_regular<R: Read + Seek>(reader: &mut R) -> Result<(CrossRefTable, Object)> {
        // Find xref table offset
        let xref_offset = find_xref_offset(reader)?;

        // Parse xref table
        let xref = parse_xref(reader, xref_offset)?;

        // Get trailer dictionary
        let trailer = if let Some(trailer_dict) = xref.trailer() {
            // XRef stream: trailer is already in the xref table
            Object::Dictionary(trailer_dict.clone())
        } else {
            // Traditional xref: parse trailer separately
            reader.seek(SeekFrom::Start(xref_offset))?;
            parse_trailer(reader)?
        };

        Ok((xref, trailer))
    }

    /// Try to reconstruct the xref table by scanning the file.
    fn try_reconstruct_xref<R: Read + Seek>(reader: &mut R) -> Result<(CrossRefTable, Object)> {
        crate::xref_reconstruction::reconstruct_xref(reader)
    }

    /// Initialize encryption handler lazily if PDF is encrypted.
    ///
    /// PDF Spec: Section 7.6.1 - Encryption dictionary in trailer
    ///
    /// This checks for the /Encrypt entry in the trailer, loads it if it's a
    /// reference, and creates an encryption handler. It automatically attempts
    /// to authenticate with an empty password (common for PDFs with default encryption).
    ///
    /// This is called lazily the first time we need to decrypt something, after
    /// the document is fully constructed and can load objects.
    fn ensure_encryption_initialized(&mut self) -> Result<()> {
        // Already initialized?
        if self.encryption_handler.is_some() {
            return Ok(());
        }

        // Clone what we need from trailer to avoid borrow conflicts
        let (encrypt_ref, file_id) = {
            let trailer_dict = match self.trailer.as_dict() {
                Some(d) => d,
                None => return Ok(()), // No trailer dict, no encryption
            };

            // Check for /Encrypt entry
            let encrypt_entry = match trailer_dict.get("Encrypt") {
                Some(obj) => obj,
                None => {
                    log::debug!("PDF is not encrypted (no /Encrypt entry)");
                    return Ok(());
                },
            };

            // Clone the encrypt entry (we'll load it outside this block)
            let encrypt_ref = encrypt_entry.clone();

            // Get file ID (required for encryption key derivation)
            let file_id = match trailer_dict.get("ID") {
                Some(Object::Array(arr)) => {
                    if let Some(first_id) = arr.first() {
                        if let Some(id_bytes) = first_id.as_string() {
                            id_bytes.to_vec()
                        } else {
                            log::warn!(
                                "Invalid /ID array entry (not a string), using empty file ID"
                            );
                            vec![]
                        }
                    } else {
                        log::warn!("Empty /ID array, using empty file ID");
                        vec![]
                    }
                },
                _ => {
                    log::warn!("Missing or invalid /ID entry in trailer, using empty file ID");
                    vec![]
                },
            };

            (encrypt_ref, file_id)
        }; // End of borrow scope

        // Now load the encrypt object (dereference if needed)
        let encrypt_obj = match encrypt_ref {
            Object::Dictionary(_) => encrypt_ref,
            Object::Reference(obj_ref) => {
                log::debug!("Loading /Encrypt object reference {} {}", obj_ref.id, obj_ref.gen);
                self.load_object(obj_ref)?
            },
            _ => {
                return Err(Error::InvalidPdf(format!(
                    "Invalid /Encrypt entry type: {}",
                    encrypt_ref.type_name()
                )));
            },
        };

        // Create encryption handler with the file_id we extracted above
        let mut handler = EncryptionHandler::new(&encrypt_obj, file_id)?;

        // Try to authenticate with empty password (common default)
        match handler.authenticate(b"") {
            Ok(true) => {
                log::info!("Successfully authenticated with empty password");
            },
            Ok(false) => {
                log::warn!("PDF is encrypted and requires a password");
                // Set handler anyway - user can call authenticate() later
            },
            Err(e) => {
                log::error!("Failed to initialize encryption: {}", e);
                return Err(e);
            },
        }

        self.encryption_handler = Some(handler);
        Ok(())
    }

    /// Decode stream data with encryption support.
    ///
    /// This is a helper method that decodes stream data using the PDF's encryption handler
    /// if the document is encrypted. It automatically handles object-specific key derivation.
    ///
    /// # Arguments
    ///
    /// * `stream_obj` - The stream object to decode
    /// * `obj_ref` - The object reference (for encryption key derivation)
    ///
    /// # Returns
    ///
    /// The decoded (and decrypted if needed) stream data.
    ///
    /// # PDF Spec Reference
    ///
    /// ISO 32000-1:2008, Section 7.6.2 - Streams must be decrypted BEFORE applying filters.
    pub(crate) fn decode_stream_with_encryption(
        &self,
        stream_obj: &Object,
        obj_ref: ObjectRef,
    ) -> Result<Vec<u8>> {
        if matches!(stream_obj, Object::Null) {
            return Ok(Vec::new());
        }
        if let Some(handler) = &self.encryption_handler {
            // Create decryption closure for this specific object
            let decrypt_fn = |data: &[u8]| -> Result<Vec<u8>> {
                handler.decrypt_stream(data, obj_ref.id, obj_ref.gen as u32)
            };
            stream_obj.decode_stream_data_with_decryption(
                Some(&decrypt_fn),
                obj_ref.id,
                obj_ref.gen as u32,
            )
        } else {
            // No encryption, use regular decoding
            stream_obj.decode_stream_data()
        }
    }

    /// Open with custom extraction profile.
    ///
    /// Currently, the profile is not used at the document level but is reserved
    /// for future integration with document-type-specific extraction settings.
    pub fn open_with_config(path: impl AsRef<Path>, _config: impl std::any::Any) -> Result<Self> {
        Self::open(path)
    }

    /// Get the PDF version.
    ///
    /// Returns a tuple (major, minor) representing the PDF version.
    /// For example, PDF 1.7 returns (1, 7).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # let mut doc = PdfDocument::open("sample.pdf")?;
    /// let (major, minor) = doc.version();
    /// println!("PDF version: {}.{}", major, minor);
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn version(&self) -> (u8, u8) {
        self.version
    }

    /// Get a reference to the trailer dictionary.
    ///
    /// The trailer dictionary contains important document metadata including:
    /// - /Root: Reference to the catalog dictionary
    /// - /Info: Reference to the document info dictionary (optional)
    /// - /Size: Number of entries in the cross-reference table
    /// - /Encrypt: Encryption dictionary (if encrypted)
    /// - /ID: File identifier array
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # let mut doc = PdfDocument::open("sample.pdf")?;
    /// let trailer = doc.trailer();
    /// if let Some(dict) = trailer.as_dict() {
    ///     if let Some(info_ref) = dict.get("Info") {
    ///         println!("Document has an Info dictionary");
    ///     }
    /// }
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn trailer(&self) -> &Object {
        &self.trailer
    }

    /// Scan the file to find an object by its header.
    ///
    /// This is a fallback method used when an object is not in the xref table
    /// but is referenced by critical structures (like Pages from Catalog).
    /// Some PDFs have incomplete xref tables that are missing entries for
    /// objects that actually exist in the file.
    fn scan_for_object(&mut self, obj_ref: ObjectRef) -> Result<u64> {
        log::info!(
            "Scanning file for object {} {} obj (not in xref table)",
            obj_ref.id,
            obj_ref.gen
        );

        // Seek to start of file
        self.reader.seek(SeekFrom::Start(0))?;

        // Read entire file into buffer for searching
        let mut content = Vec::new();
        self.reader.read_to_end(&mut content)?;

        // Build search pattern: "\n{id} {gen} obj" or "\r{id} {gen} obj"
        let pattern = format!("{} {} obj", obj_ref.id, obj_ref.gen);
        let pattern_bytes = pattern.as_bytes();

        // Search for the pattern
        let mut pos = 0;
        while pos < content.len() {
            if let Some(relative_pos) = content[pos..]
                .windows(pattern_bytes.len())
                .position(|w| w == pattern_bytes)
            {
                let absolute_pos = pos + relative_pos;

                // Check if preceded by newline or start of file
                let valid_start = if absolute_pos == 0 {
                    true
                } else {
                    let prev_char = content[absolute_pos - 1];
                    prev_char == b'\n' || prev_char == b'\r'
                };

                // Check if followed by whitespace, newline, or '<' (start of dictionary)
                // PDF allows "N G obj<<..." with no space
                let end_pos = absolute_pos + pattern_bytes.len();
                let valid_end = if end_pos >= content.len() {
                    true
                } else {
                    let next_char = content[end_pos];
                    next_char == b'\n'
                        || next_char == b'\r'
                        || next_char == b' '
                        || next_char == b'\t'
                        || next_char == b'<'
                };

                if valid_start && valid_end {
                    // Found it! The object header starts at absolute_pos
                    // (We already validated it's preceded by newline or is at start of file)
                    log::info!(
                        "Found object {} {} obj at byte offset {} (scanned file)",
                        obj_ref.id,
                        obj_ref.gen,
                        absolute_pos
                    );
                    return Ok(absolute_pos as u64);
                }

                pos = absolute_pos + 1;
            } else {
                break;
            }
        }

        Err(Error::ObjectNotFound(obj_ref.id, obj_ref.gen))
    }

    /// Load an object by its reference.
    ///
    /// This function:
    /// 1. Checks the object cache first
    /// 2. If not cached, looks up the byte offset in the xref table
    /// 3. Seeks to that offset and parses the object
    /// 4. Caches the result for future access
    /// 5. If object not in xref but is critical, scans file for it
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The object reference is not in the xref table and file scan fails
    /// - The object is not in use (free object)
    /// - Seeking to the object offset fails
    /// - Parsing the object fails
    /// - A circular reference is detected
    /// - The recursion depth limit is exceeded
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # use pdf_oxide::object::ObjectRef;
    /// # let mut doc = PdfDocument::open("sample.pdf")?;
    /// let obj_ref = ObjectRef::new(1, 0);
    /// let obj = doc.load_object(obj_ref)?;
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn load_object(&mut self, obj_ref: ObjectRef) -> Result<Object> {
        log::debug!("Loading object {} gen {}", obj_ref.id, obj_ref.gen);

        // Check recursion depth
        {
            let depth = *self.recursion_depth.borrow();
            if depth >= MAX_RECURSION_DEPTH {
                log::error!(
                    "Recursion depth limit exceeded ({}) while loading object {} gen {}",
                    MAX_RECURSION_DEPTH,
                    obj_ref.id,
                    obj_ref.gen
                );
                return Err(Error::RecursionLimitExceeded(MAX_RECURSION_DEPTH));
            }
        }

        // Check for circular references
        if self.resolving_stack.borrow().contains(&obj_ref) {
            log::error!(
                "Circular reference detected for object {} gen {} (depth: {})",
                obj_ref.id,
                obj_ref.gen,
                self.recursion_depth.borrow()
            );
            return Err(Error::CircularReference(obj_ref));
        }

        // Check cache first
        if let Some(cached) = self.object_cache.get(&obj_ref) {
            log::debug!("  → Found in cache");
            return Ok(cached.clone());
        }

        // Look up in xref table
        let entry = match self.xref.get(obj_ref.id) {
            Some(entry) => entry,
            None => {
                // Object not in xref table - try scanning the file as fallback
                // This handles PDFs with incomplete/corrupted xref tables
                let available: Vec<u32> = self.xref.entries.keys().copied().take(20).collect();
                log::warn!(
                    "Object {} not in xref table. Total entries: {}. First 20 objects: {:?}",
                    obj_ref.id,
                    self.xref.len(),
                    available
                );

                // Try to scan the file for this object
                match self.scan_for_object(obj_ref) {
                    Ok(offset) => {
                        // Found it! Load directly from this offset
                        log::info!(
                            "Successfully found object {} via file scan at offset {}",
                            obj_ref.id,
                            offset
                        );

                        // Mark as being resolved (cycle detection)
                        self.resolving_stack.borrow_mut().insert(obj_ref);

                        // Increment recursion depth
                        *self.recursion_depth.borrow_mut() += 1;

                        // Load the object
                        let result = self.load_uncompressed_object(obj_ref, offset);

                        // Decrement recursion depth
                        *self.recursion_depth.borrow_mut() -= 1;

                        // Unmark when done
                        self.resolving_stack.borrow_mut().remove(&obj_ref);

                        return result;
                    },
                    Err(_) => {
                        // File scan also failed
                        return Err(Error::ObjectNotFound(obj_ref.id, obj_ref.gen));
                    },
                }
            },
        };

        log::debug!(
            "  → Found in xref: type={:?}, offset={}, gen={}, in_use={}",
            entry.entry_type,
            entry.offset,
            entry.generation,
            entry.in_use
        );

        // Check if object is in use
        if !entry.in_use {
            log::warn!(
                "Object {} is marked as free (not in use). This may be due to a corrupted xref table.",
                obj_ref.id
            );

            // For critical objects like catalog/root, try to find them by scanning
            // rather than immediately failing
            if obj_ref.id <= 10 {
                log::info!(
                    "Object {} is a low-numbered object (likely critical), attempting fallback lookup",
                    obj_ref.id
                );
                // TODO: Implement file scanning fallback
                // For now, try loading anyway if offset looks reasonable
                if entry.offset > 0 && entry.offset < 100_000_000 {
                    log::info!(
                        "Attempting to load object {} from offset {} despite free status",
                        obj_ref.id,
                        entry.offset
                    );
                    // Fall through to loading logic below
                } else {
                    return Err(Error::ObjectNotFound(obj_ref.id, obj_ref.gen));
                }
            } else {
                return Err(Error::ObjectNotFound(obj_ref.id, obj_ref.gen));
            }
        }

        // Mark as being resolved (cycle detection)
        self.resolving_stack.borrow_mut().insert(obj_ref);

        // Increment recursion depth
        *self.recursion_depth.borrow_mut() += 1;

        // Handle different entry types
        use crate::xref::XRefEntryType;
        let result = match entry.entry_type {
            XRefEntryType::Compressed => {
                // Type 2 entry: object is in an object stream
                // entry.offset = stream object number
                // entry.generation = index within stream
                log::debug!(
                    "  → Compressed object in stream {}, index {}",
                    entry.offset,
                    entry.generation
                );
                self.load_compressed_object(obj_ref, entry.offset as u32, entry.generation)
            },
            XRefEntryType::Uncompressed => {
                // Type 1 entry: traditional uncompressed object
                log::debug!("  → Uncompressed object at offset {}", entry.offset);
                self.load_uncompressed_object(obj_ref, entry.offset)
            },
            XRefEntryType::Free => {
                // Free object - shouldn't happen since we check in_use above
                log::warn!("Object {} has type Free despite in_use=true", obj_ref.id);
                Err(Error::ObjectNotFound(obj_ref.id, obj_ref.gen))
            },
        };

        // Decrement recursion depth
        *self.recursion_depth.borrow_mut() -= 1;

        // Unmark when done
        self.resolving_stack.borrow_mut().remove(&obj_ref);

        result
    }

    /// Resolve references within an object recursively.
    ///
    /// This utility method resolves indirect references within an object,
    /// handling nested dictionaries and arrays up to a specified depth.
    /// Useful for processing complex PDF structures where properties
    /// may be stored as indirect references.
    ///
    /// # Arguments
    ///
    /// * `obj` - The object to resolve references within
    /// * `max_depth` - Maximum recursion depth to prevent infinite loops
    ///
    /// # Returns
    ///
    /// The object with all references resolved up to max_depth levels.
    /// If a reference cannot be resolved, it is left as-is.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # let mut doc = PdfDocument::open("sample.pdf")?;
    /// # let obj = doc.catalog()?;
    /// // Resolve all references in a dictionary up to 3 levels deep
    /// let resolved = doc.resolve_references(&obj, 3)?;
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn resolve_references(&mut self, obj: &Object, max_depth: usize) -> Result<Object> {
        if max_depth == 0 {
            return Ok(obj.clone());
        }

        match obj {
            Object::Reference(obj_ref) => {
                // Resolve the reference
                match self.load_object(*obj_ref) {
                    Ok(resolved) => {
                        // Recursively resolve within the resolved object
                        self.resolve_references(&resolved, max_depth - 1)
                    },
                    Err(e) => {
                        log::warn!("Failed to resolve reference {:?}: {}", obj_ref, e);
                        Ok(obj.clone()) // Return the unresolved reference
                    },
                }
            },

            Object::Dictionary(dict) => {
                // Resolve references within each value
                let mut resolved_dict = std::collections::HashMap::new();
                for (key, value) in dict.iter() {
                    let resolved_value = self.resolve_references(value, max_depth - 1)?;
                    resolved_dict.insert(key.clone(), resolved_value);
                }
                Ok(Object::Dictionary(resolved_dict))
            },

            Object::Array(arr) => {
                // Resolve references within each element
                let resolved_arr: Result<Vec<Object>> = arr
                    .iter()
                    .map(|item| self.resolve_references(item, max_depth - 1))
                    .collect();
                Ok(Object::Array(resolved_arr?))
            },

            // For all other types, just return a clone
            _ => Ok(obj.clone()),
        }
    }

    /// Load an uncompressed object (Type 1 xref entry).
    fn load_uncompressed_object(&mut self, obj_ref: ObjectRef, offset: u64) -> Result<Object> {
        self.load_uncompressed_object_impl(obj_ref, offset, false)
    }

    /// Implementation with recursion guard to prevent infinite loops.
    fn load_uncompressed_object_impl(
        &mut self,
        obj_ref: ObjectRef,
        offset: u64,
        already_corrected: bool,
    ) -> Result<Object> {
        // Seek to object offset
        self.reader.seek(SeekFrom::Start(offset))?;

        // Read bytes for object header (e.g., "1 0 obj")
        // Use bytes instead of String to handle binary data gracefully
        let mut header_bytes = Vec::new();
        let bytes_read = self.reader.read_until(b'\n', &mut header_bytes)?;

        if bytes_read == 0 {
            log::warn!("Unexpected EOF while reading object {} header", obj_ref.id);
            return Err(Error::UnexpectedEof);
        }

        // Try to parse as UTF-8, but handle binary data gracefully
        let line = String::from_utf8_lossy(&header_bytes);

        // Issue #45: Handle multi-line object headers
        // Some PDFs split the header across multiple lines (e.g., "1\n0\nobj")
        // Read additional lines until we have a complete header
        let mut full_header = line.to_string();
        let max_header_lines = 5; // Reasonable limit to avoid infinite loops
        let mut lines_read = 1;

        while !full_header.contains("obj") && lines_read < max_header_lines {
            let mut next_bytes = Vec::new();
            let next_read = self.reader.read_until(b'\n', &mut next_bytes)?;

            if next_read == 0 {
                break; // EOF reached
            }

            let next_line = String::from_utf8_lossy(&next_bytes);
            full_header.push(' ');
            full_header.push_str(&next_line);
            lines_read += 1;
        }

        // Verify object header format
        // Split by whitespace to handle various formats (single-line or multi-line)
        let parts: Vec<&str> = full_header.split_whitespace().collect();

        // Find "obj" keyword position
        let obj_pos = parts.iter().position(|&p| p == "obj" || p.contains("obj"));

        // Validate object header has proper format: <id> <gen> obj
        let obj_pos = match obj_pos {
            Some(pos) if pos >= 2 => pos,
            _ => {
                // Only try backwards search once to prevent infinite recursion
                if !already_corrected {
                    // xref offset might be incorrect (pointing to object body instead of header)
                    // Try searching backwards for the object header
                    log::debug!(
                        "No object header at offset {}, searching backwards for object {} {} obj",
                        offset,
                        obj_ref.id,
                        obj_ref.gen
                    );

                    if let Ok(corrected_offset) = self.find_object_header_backwards(obj_ref, offset)
                    {
                        log::info!(
                            "Found object header at offset {} (xref said {})",
                            corrected_offset,
                            offset
                        );
                        return self.load_uncompressed_object_impl(obj_ref, corrected_offset, true);
                    }
                }

                log::warn!("Malformed object header at offset {}: {}", offset, line.trim());
                return Err(Error::ParseError {
                    offset: offset as usize,
                    reason: format!("Expected object header, found: {}", line.trim()),
                });
            },
        };

        let _obj_pos = obj_pos;

        // Parse the object number and generation from header
        let obj_num: u32 = parts[0].parse().map_err(|_| Error::ParseError {
            offset: offset as usize,
            reason: format!("Invalid object number in header: {}", parts[0]),
        })?;
        let gen_num: u16 = parts[1].parse().map_err(|_| Error::ParseError {
            offset: offset as usize,
            reason: format!("Invalid generation number in header: {}", parts[1]),
        })?;

        // Verify object reference matches (warn but don't fail on mismatch)
        if obj_num != obj_ref.id || gen_num != obj_ref.gen {
            log::warn!(
                "Object reference mismatch at offset {}: expected {} {} obj, found {} {} obj",
                offset,
                obj_ref.id,
                obj_ref.gen,
                obj_num,
                gen_num
            );
        }

        // Check if there's content after "obj" on the same line
        // Some PDFs have "N G obj\n<<..." while others have "N G obj<<..." on one line
        let mut data = Vec::new();

        // Find where "obj" ends in the original bytes
        // We need to include anything after "obj" in the header line
        if let Some(obj_keyword_pos) = header_bytes.windows(3).position(|w| w == b"obj") {
            let after_obj_pos = obj_keyword_pos + 3; // "obj" is 3 bytes

            // Skip whitespace after "obj"
            let mut content_start = after_obj_pos;
            while content_start < header_bytes.len()
                && (header_bytes[content_start] == b' '
                    || header_bytes[content_start] == b'\t'
                    || header_bytes[content_start] == b'\r')
            {
                content_start += 1;
            }

            // If there's a newline, skip it (normal case: "N G obj\n")
            // If there's content (like "<<"), include it (malformed case: "N G obj<<...")
            if content_start < header_bytes.len() && header_bytes[content_start] != b'\n' {
                // There's content on the same line after "obj" - include it
                data.extend_from_slice(&header_bytes[content_start..]);
                log::debug!(
                    "Object {} has content after 'obj' on header line ({} bytes)",
                    obj_ref.id,
                    header_bytes.len() - content_start
                );
            }
        }

        // Read the rest of the object data until "endobj"
        let mut lines_read = 0;
        const MAX_LINES: usize = 10000; // Prevent infinite loops

        loop {
            let mut chunk = Vec::new();
            let bytes_read = self.reader.read_until(b'\n', &mut chunk)?;

            lines_read += 1;
            if lines_read > MAX_LINES {
                log::warn!(
                    "Object {} exceeded maximum line count ({}), truncating",
                    obj_ref.id,
                    MAX_LINES
                );
                break;
            }

            if bytes_read == 0 {
                log::warn!(
                    "Unexpected EOF while reading object {} (no endobj found after {} lines)",
                    obj_ref.id,
                    lines_read
                );
                // Don't fail - try to parse what we have
                break;
            }

            // Check if we reached endobj
            if chunk.contains(&b'e') {
                // Find "endobj" in the chunk (working with bytes, not chars)
                if let Some(endobj_pos) = find_substring(&chunk, b"endobj") {
                    // Include everything before "endobj" but not "endobj" itself
                    data.extend_from_slice(&chunk[..endobj_pos]);
                    break;
                }
            }

            data.extend_from_slice(&chunk);
        }

        // Parse the object data
        log::debug!(
            "About to parse object {} gen {} ({} bytes)",
            obj_ref.id,
            obj_ref.gen,
            data.len()
        );

        // Phase 6B: Graceful degradation for corrupted objects
        // Instead of failing on parse errors, return Null placeholder
        // This allows partial content extraction from PDFs with truncated objects
        let obj = match parse_object(&data) {
            Ok((_, parsed_obj)) => parsed_obj,
            Err(e) => {
                // Extract error kind without printing raw bytes
                let error_kind = match &e {
                    nom::Err::Incomplete(_) => "Incomplete data",
                    nom::Err::Error(err) | nom::Err::Failure(err) => match err.code {
                        nom::error::ErrorKind::Eof => "Unexpected EOF",
                        nom::error::ErrorKind::Tag => "Expected tag not found",
                        nom::error::ErrorKind::Fail => "Parse failed",
                        _ => "Parse error",
                    },
                };
                log::warn!(
                    "Object {} at offset {} is corrupted ({}), using Null placeholder. \
                     This may result in missing content from the PDF.",
                    obj_ref.id,
                    offset,
                    error_kind
                );
                // Return Null object instead of failing
                // This allows extraction to continue with partial content
                Object::Null
            },
        };

        // Cache the object
        self.object_cache.insert(obj_ref, obj.clone());

        Ok(obj)
    }

    /// Load a compressed object from an object stream (Type 2 xref entry).
    ///
    /// # Arguments
    ///
    /// * `obj_ref` - The object reference being loaded
    /// * `stream_obj_num` - The object number of the object stream
    /// * `index_in_stream` - The index within the stream (unused but provided for completeness)
    fn load_compressed_object(
        &mut self,
        obj_ref: ObjectRef,
        stream_obj_num: u32,
        _index_in_stream: u16,
    ) -> Result<Object> {
        use crate::objstm::parse_object_stream_with_decryption;

        // Ensure encryption is initialized if needed (lazy initialization)
        self.ensure_encryption_initialized()?;

        // Load the object stream
        let stream_ref = ObjectRef::new(stream_obj_num, 0);
        let stream_obj = self.load_uncompressed_object(stream_ref, {
            // Look up the stream's offset in the xref table
            let stream_entry = self
                .xref
                .get(stream_obj_num)
                .ok_or(Error::ObjectNotFound(stream_obj_num, 0))?;

            if stream_entry.entry_type != crate::xref::XRefEntryType::Uncompressed {
                return Err(Error::InvalidPdf(format!(
                    "object stream {} is not an uncompressed object",
                    stream_obj_num
                )));
            }

            stream_entry.offset
        })?;

        // Parse all objects from the stream (with decryption if PDF is encrypted)
        let objects_map = if let Some(handler) = &self.encryption_handler {
            // Create decryption closure
            let decrypt_fn = |data: &[u8]| -> Result<Vec<u8>> {
                handler.decrypt_stream(data, stream_obj_num, 0)
            };
            parse_object_stream_with_decryption(&stream_obj, Some(&decrypt_fn), stream_obj_num, 0)?
        } else {
            parse_object_stream_with_decryption(&stream_obj, None, 0, 0)?
        };

        // Extract the requested object
        let obj = objects_map
            .get(&obj_ref.id)
            .ok_or(Error::ObjectNotFound(obj_ref.id, obj_ref.gen))?
            .clone();

        // Cache all objects from the stream for future access
        for (obj_num, object) in objects_map {
            let cache_ref = ObjectRef::new(obj_num, 0);
            self.object_cache.insert(cache_ref, object);
        }

        Ok(obj)
    }

    /// Find object header by searching backwards from a given offset.
    ///
    /// Some PDF generators create xref tables with incorrect offsets that point
    /// to the object body instead of the header. This function searches backwards
    /// from the xref offset to find the actual "N G obj" header.
    ///
    /// We search up to 100 bytes backwards, looking for a line that matches
    /// the expected object header format.
    fn find_object_header_backwards(
        &mut self,
        obj_ref: ObjectRef,
        wrong_offset: u64,
    ) -> Result<u64> {
        // Don't search before the start of the file
        if wrong_offset == 0 {
            return Err(Error::ParseError {
                offset: wrong_offset as usize,
                reason: "Cannot search backwards from offset 0".to_string(),
            });
        }

        // Search up to 100 bytes backwards (reasonable for most PDFs)
        let search_distance = std::cmp::min(100, wrong_offset);
        let search_start = wrong_offset - search_distance;

        // Read the search region
        self.reader.seek(SeekFrom::Start(search_start))?;
        let mut buffer = vec![0u8; search_distance as usize + 100]; // Extra bytes to read full line
        let bytes_read = self.reader.read(&mut buffer)?;

        if bytes_read == 0 {
            return Err(Error::ParseError {
                offset: wrong_offset as usize,
                reason: "Could not read backwards search region".to_string(),
            });
        }

        // Build the expected header pattern as bytes (NOT string to avoid UTF-8 corruption)
        let expected_header = format!("{} {} obj", obj_ref.id, obj_ref.gen);
        let pattern_bytes = expected_header.as_bytes();

        // Search for the byte pattern directly (avoids UTF-8 conversion issues with binary data)
        // Find the match closest to wrong_offset (prefer before, but allow small offsets after)
        let mut best_match: Option<(usize, i64)> = None; // (position, distance_from_wrong)

        for (i, window) in buffer[..bytes_read]
            .windows(pattern_bytes.len())
            .enumerate()
        {
            if window == pattern_bytes {
                let candidate_offset = search_start + i as u64;
                let distance = (candidate_offset as i64) - (wrong_offset as i64);

                // Accept matches within -100 to +10 bytes of wrong_offset
                // (xref might be slightly off by a few bytes)
                if (-100..=10).contains(&distance) {
                    // Prefer the match closest to wrong_offset
                    let is_better = best_match
                        .as_ref()
                        .is_none_or(|(_, best_dist)| distance.abs() < best_dist.abs());

                    if is_better {
                        best_match = Some((i, distance));
                    }
                }
            }
        }

        if let Some((pos, distance)) = best_match {
            let absolute_offset = search_start + pos as u64;
            log::debug!(
                "Found object header '{}' at offset {} ({:+} bytes from xref at {})",
                expected_header,
                absolute_offset,
                distance,
                wrong_offset
            );
            return Ok(absolute_offset);
        }

        // Try with whitespace variations (space, double-space, tab between obj_id and gen)
        let patterns = [
            format!("{} {} obj", obj_ref.id, obj_ref.gen).into_bytes(),
            format!("{}  {} obj", obj_ref.id, obj_ref.gen).into_bytes(),
            format!("{}\t{} obj", obj_ref.id, obj_ref.gen).into_bytes(),
            format!("{} {}\tobj", obj_ref.id, obj_ref.gen).into_bytes(),
        ];

        for pattern in &patterns {
            let mut best_match: Option<(usize, i64)> = None;

            for (i, window) in buffer[..bytes_read].windows(pattern.len()).enumerate() {
                if window == pattern.as_slice() {
                    let candidate_offset = search_start + i as u64;
                    let distance = (candidate_offset as i64) - (wrong_offset as i64);

                    if (-100..=10).contains(&distance) {
                        let is_better = best_match
                            .as_ref()
                            .is_none_or(|(_, best_dist)| distance.abs() < best_dist.abs());

                        if is_better {
                            best_match = Some((i, distance));
                        }
                    }
                }
            }

            if let Some((pos, distance)) = best_match {
                let absolute_offset = search_start + pos as u64;
                log::debug!(
                    "Found object header '{}' at offset {} ({:+} bytes, pattern match)",
                    expected_header,
                    absolute_offset,
                    distance
                );
                return Ok(absolute_offset);
            }
        }

        Err(Error::ParseError {
            offset: wrong_offset as usize,
            reason: format!(
                "Could not find object header '{}' within {} bytes before offset",
                expected_header, search_distance
            ),
        })
    }

    /// Get the document catalog (root object).
    ///
    /// The catalog is the root of the document's object hierarchy.
    /// It contains references to the page tree, outlines, etc.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The trailer does not contain a /Root entry
    /// - The /Root entry is not a reference
    /// - Loading the catalog object fails
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # let mut doc = PdfDocument::open("sample.pdf")?;
    /// let catalog = doc.catalog()?;
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn catalog(&mut self) -> Result<Object> {
        let trailer_dict = self
            .trailer
            .as_dict()
            .ok_or_else(|| Error::InvalidPdf("Trailer is not a dictionary".to_string()))?;

        let root_ref = trailer_dict
            .get("Root")
            .ok_or_else(|| Error::InvalidPdf("Trailer missing /Root entry".to_string()))?
            .as_reference()
            .ok_or_else(|| Error::InvalidPdf("/Root is not a reference".to_string()))?;

        self.load_object(root_ref)
    }

    /// Get the structure tree (logical structure) of the document.
    ///
    /// Tagged PDFs contain a structure tree that defines the logical structure
    /// and reading order of the document. This is the PDF-spec-compliant way
    /// to determine reading order.
    ///
    /// Returns `Ok(Some(StructTreeRoot))` if the document has a structure tree,
    /// `Ok(None)` if it's not a tagged PDF, or an error if parsing fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # let mut doc = PdfDocument::open("sample.pdf")?;
    /// if let Some(struct_tree) = doc.structure_tree()? {
    ///     println!("This is a Tagged PDF with logical structure");
    /// } else {
    ///     println!("This PDF does not have a structure tree");
    /// }
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn structure_tree(&mut self) -> Result<Option<crate::structure::StructTreeRoot>> {
        crate::structure::parse_structure_tree(self)
    }

    /// Get the MarkInfo dictionary from the document catalog.
    ///
    /// The MarkInfo dictionary indicates whether the document conforms to
    /// Tagged PDF conventions and whether the structure tree might contain
    /// suspect (unreliable) content.
    ///
    /// Per ISO 32000-1:2008 Section 14.7.1, the MarkInfo dictionary contains:
    /// - `/Marked` - Whether the document conforms to Tagged PDF conventions
    /// - `/Suspects` - Whether the document contains suspect content
    /// - `/UserProperties` - Whether the document uses user properties
    ///
    /// # Returns
    ///
    /// Returns `MarkInfo` with the parsed values, or default values if
    /// the MarkInfo dictionary is not present.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # let mut doc = PdfDocument::open("sample.pdf")?;
    /// let mark_info = doc.mark_info()?;
    /// if mark_info.is_structure_reliable() {
    ///     println!("Structure tree can be trusted for reading order");
    /// } else if mark_info.suspects {
    ///     println!("Structure tree may contain unreliable content");
    /// }
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn mark_info(&mut self) -> Result<crate::structure::MarkInfo> {
        let catalog = self.catalog()?;
        let catalog_dict = match catalog.as_dict() {
            Some(d) => d,
            None => return Ok(crate::structure::MarkInfo::default()),
        };

        // Get /MarkInfo dictionary
        let mark_info_obj = match catalog_dict.get("MarkInfo") {
            Some(obj) => obj,
            None => return Ok(crate::structure::MarkInfo::default()),
        };

        // Resolve reference if needed
        let mark_info_obj = if let Some(r) = mark_info_obj.as_reference() {
            self.load_object(r)?
        } else {
            mark_info_obj.clone()
        };

        let mark_info_dict = match mark_info_obj.as_dict() {
            Some(d) => d,
            None => return Ok(crate::structure::MarkInfo::default()),
        };

        // Parse boolean fields with defaults of false
        let marked = mark_info_dict
            .get("Marked")
            .and_then(|o: &crate::object::Object| o.as_bool())
            .unwrap_or(false);

        let suspects = mark_info_dict
            .get("Suspects")
            .and_then(|o: &crate::object::Object| o.as_bool())
            .unwrap_or(false);

        let user_properties = mark_info_dict
            .get("UserProperties")
            .and_then(|o: &crate::object::Object| o.as_bool())
            .unwrap_or(false);

        Ok(crate::structure::MarkInfo {
            marked,
            suspects,
            user_properties,
        })
    }

    /// Get the number of pages in the document.
    ///
    /// This function:
    /// 1. Loads the catalog (root object)
    /// 2. Follows the /Pages reference to the page tree root
    /// 3. Extracts the /Count value from the page tree
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The catalog cannot be loaded
    /// - The /Pages entry is missing or invalid
    /// - The page tree root does not contain a /Count entry
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # let mut doc = PdfDocument::open("sample.pdf")?;
    /// let count = doc.page_count()?;
    /// println!("Document has {} pages", count);
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn page_count(&mut self) -> Result<usize> {
        // Try standard method first
        match self.get_page_count_standard() {
            Ok(count) => {
                log::debug!("Page count from /Count: {}", count);
                Ok(count)
            },
            Err(e) => {
                log::warn!("Failed to get page count from /Count: {}", e);
                log::info!("Falling back to scanning page tree");

                // Fallback: scan the page tree manually
                match self.get_page_count_by_scanning() {
                    Ok(count) => {
                        log::info!("Page count from scanning: {}", count);
                        Ok(count)
                    },
                    Err(scan_err) => {
                        log::error!("Both methods failed. Standard: {}, Scan: {}", e, scan_err);
                        Err(e) // Return original error
                    },
                }
            },
        }
    }

    /// Get page count using the standard /Count field
    fn get_page_count_standard(&mut self) -> Result<usize> {
        // Load catalog
        let catalog = self.catalog()?;
        let catalog_dict = catalog.as_dict().ok_or_else(|| Error::InvalidObjectType {
            expected: "Dictionary".to_string(),
            found: "Other".to_string(),
        })?;

        // Get /Pages reference
        let pages_ref = catalog_dict
            .get("Pages")
            .ok_or_else(|| Error::InvalidPdf("Catalog missing /Pages entry".to_string()))?
            .as_reference()
            .ok_or_else(|| Error::InvalidPdf("/Pages is not a reference".to_string()))?;

        // Load page tree root
        let pages_obj = self.load_object(pages_ref)?;
        let pages_dict = pages_obj
            .as_dict()
            .ok_or_else(|| Error::InvalidObjectType {
                expected: "Dictionary".to_string(),
                found: "Other".to_string(),
            })?;

        // Get /Count
        let count = pages_dict
            .get("Count")
            .ok_or_else(|| Error::InvalidPdf("Page tree missing /Count entry".to_string()))?
            .as_integer()
            .ok_or_else(|| Error::InvalidPdf("/Count is not an integer".to_string()))?;

        Ok(count as usize)
    }

    /// Get page count by scanning the page tree (fallback method)
    fn get_page_count_by_scanning(&mut self) -> Result<usize> {
        // Load catalog
        let catalog = self.catalog()?;
        let catalog_dict = catalog.as_dict().ok_or_else(|| Error::InvalidObjectType {
            expected: "Dictionary".to_string(),
            found: "Other".to_string(),
        })?;

        // Get /Pages reference
        let pages_ref = catalog_dict
            .get("Pages")
            .ok_or_else(|| Error::InvalidPdf("Catalog missing /Pages entry".to_string()))?
            .as_reference()
            .ok_or_else(|| Error::InvalidPdf("/Pages is not a reference".to_string()))?;

        // Count pages by traversing the tree
        self.count_pages_recursive(pages_ref, 0)
    }

    /// Recursively count pages in the page tree
    fn count_pages_recursive(&mut self, node_ref: ObjectRef, depth: usize) -> Result<usize> {
        // Prevent infinite recursion
        const MAX_DEPTH: usize = 50;
        if depth > MAX_DEPTH {
            log::warn!("Page tree depth exceeded {} levels, stopping", MAX_DEPTH);
            return Ok(0);
        }

        // Load the node
        let node = match self.load_object(node_ref) {
            Ok(n) => n,
            Err(e) => {
                log::warn!("Failed to load page tree node {}: {}", node_ref, e);
                return Ok(0); // Skip this node
            },
        };

        let node_dict = match node.as_dict() {
            Some(d) => d,
            None => {
                log::warn!("Page tree node {} is not a dictionary", node_ref);
                return Ok(0);
            },
        };

        // Check node type
        let node_type = node_dict.get("Type").and_then(|obj| obj.as_name());

        match node_type {
            Some("Page") => {
                // This is a leaf page
                Ok(1)
            },
            Some("Pages") => {
                // This is an intermediate node with kids
                let kids = match node_dict.get("Kids").and_then(|obj| obj.as_array()) {
                    Some(k) => k,
                    None => {
                        log::warn!("Pages node {} missing /Kids array", node_ref);
                        return Ok(0);
                    },
                };

                let mut count = 0;
                for kid in kids {
                    if let Some(kid_ref) = kid.as_reference() {
                        match self.count_pages_recursive(kid_ref, depth + 1) {
                            Ok(page_count) => count += page_count,
                            Err(Error::CircularReference(obj_ref)) => {
                                log::warn!(
                                    "Circular reference in page tree at object {}, skipping",
                                    obj_ref
                                );
                                continue;
                            },
                            Err(Error::RecursionLimitExceeded(_)) => {
                                log::warn!(
                                    "Recursion limit exceeded in page tree, skipping branch"
                                );
                                continue;
                            },
                            Err(e) => {
                                log::warn!("Error counting pages in branch: {}, skipping", e);
                                continue;
                            },
                        }
                    }
                }
                Ok(count)
            },
            _ => {
                log::warn!("Unknown page tree node type: {:?}", node_type.unwrap_or("(none)"));
                Ok(0)
            },
        }
    }

    /// Get page count as u32 (legacy API).
    ///
    /// This is a convenience method that returns the page count as a u32.
    /// It calls `page_count()` internally but converts the result and
    /// returns 0 if an error occurs (for backward compatibility).
    #[deprecated(
        since = "0.1.0",
        note = "Use page_count() instead, which returns Result"
    )]
    pub fn page_count_u32(&mut self) -> u32 {
        self.page_count().unwrap_or(0) as u32
    }

    /// Get a page object by index (0-based).
    ///
    /// # Arguments
    ///
    /// * `page_index` - Zero-based page index
    ///
    /// # Returns
    ///
    /// The page dictionary object.
    ///
    /// # Errors
    ///
    /// Returns an error if the page index is out of bounds or if the page
    /// tree structure is invalid.
    fn get_page(&mut self, page_index: usize) -> Result<Object> {
        // Load catalog
        let catalog = self.catalog()?;
        let catalog_dict = catalog.as_dict().ok_or_else(|| Error::InvalidObjectType {
            expected: "Dictionary".to_string(),
            found: "Other".to_string(),
        })?;

        // Get /Pages reference
        let pages_ref = catalog_dict
            .get("Pages")
            .ok_or_else(|| Error::InvalidPdf("Catalog missing /Pages entry".to_string()))?
            .as_reference()
            .ok_or_else(|| Error::InvalidPdf("/Pages is not a reference".to_string()))?;

        // Initialize inherited attributes map
        // PDF Spec: ISO 32000-1:2008, Section 7.7.3.3
        // "An attribute of a page can be inherited from its ancestor nodes in the page tree"
        let mut inherited = HashMap::new();

        // Load page tree and find the requested page
        match self.get_page_from_tree(pages_ref, page_index, &mut 0, &mut inherited) {
            Ok(page) => Ok(page),
            Err(e) => {
                // If tree traversal fails (malformed page tree), try fallback scanning
                if matches!(
                    e,
                    Error::InvalidPdf(_)
                        | Error::InvalidObjectType { .. }
                        | Error::CircularReference(_)
                ) {
                    log::warn!("Page tree traversal failed ({}), trying fallback scan method", e);
                    self.get_page_by_scanning(page_index)
                } else {
                    Err(e)
                }
            },
        }
    }

    /// Get a page by scanning all objects in the PDF (fallback for broken page trees)
    /// This method is used when the standard page tree traversal fails due to malformed structure.
    fn get_page_by_scanning(&mut self, target_index: usize) -> Result<Object> {
        let mut current_index = 0;

        // Collect all object numbers first to avoid borrow checker issues
        // Sort for deterministic iteration order (HashMap iteration is non-deterministic)
        let mut obj_nums: Vec<u32> = self.xref.all_object_numbers().collect();
        obj_nums.sort_unstable();

        // Iterate through all objects looking for Page objects
        for obj_num in obj_nums {
            if let Ok(obj) = self.load_object(ObjectRef {
                id: obj_num,
                gen: 0,
            }) {
                if let Some(dict) = obj.as_dict() {
                    if let Some(type_obj) = dict.get("Type") {
                        if let Some(type_name) = type_obj.as_name() {
                            if type_name == "Page" {
                                if current_index == target_index {
                                    return Ok(obj);
                                }
                                current_index += 1;
                            }
                        }
                    }
                }
            }
        }

        Err(Error::InvalidPdf(format!("Page index {} not found by scanning", target_index)))
    }

    /// Recursively traverse page tree to find a specific page.
    ///
    /// PDF Spec: ISO 32000-1:2008, Section 7.7.3.3 - Page Objects
    /// Implements attribute inheritance for /Resources, /MediaBox, /CropBox, /Rotate.
    ///
    /// Inheritable attributes from parent Pages nodes are collected as we traverse down
    /// the tree. When a Page is found, inherited attributes are merged in (only if the
    /// Page doesn't already have them - child values override parent values).
    fn get_page_from_tree(
        &mut self,
        node_ref: ObjectRef,
        target_index: usize,
        current_index: &mut usize,
        inherited: &mut HashMap<String, Object>,
    ) -> Result<Object> {
        self.get_page_from_tree_inner(
            node_ref,
            target_index,
            current_index,
            inherited,
            &mut HashSet::new(),
        )
    }

    fn get_page_from_tree_inner(
        &mut self,
        node_ref: ObjectRef,
        target_index: usize,
        current_index: &mut usize,
        inherited: &mut HashMap<String, Object>,
        visited: &mut HashSet<ObjectRef>,
    ) -> Result<Object> {
        if !visited.insert(node_ref) {
            return Err(Error::CircularReference(node_ref));
        }
        let node = self.load_object(node_ref)?;
        let node_dict = node.as_dict().ok_or_else(|| Error::InvalidObjectType {
            expected: "Dictionary".to_string(),
            found: "Other".to_string(),
        })?;

        // Check if this is a page or pages node
        let node_type = node_dict
            .get("Type")
            .and_then(|obj| obj.as_name())
            .ok_or_else(|| Error::InvalidPdf("Page tree node missing /Type".to_string()))?;

        match node_type {
            "Page" => {
                // This is a leaf page
                if *current_index == target_index {
                    // Apply inherited attributes to this page
                    // PDF Spec: "If not present in the page dictionary, the value is inherited
                    // from an ancestor node in the page tree"
                    let mut page_dict = node_dict.clone();

                    // Inheritable attributes per PDF Spec Table 30:
                    // - Resources (required, can be inherited)
                    // - MediaBox (required, can be inherited)
                    // - CropBox (optional, can be inherited)
                    // - Rotate (optional, can be inherited)
                    let inheritable_attrs = ["Resources", "MediaBox", "CropBox", "Rotate"];

                    for attr_name in &inheritable_attrs {
                        // Only inherit if page doesn't already have this attribute
                        if !page_dict.contains_key(*attr_name) {
                            if let Some(inherited_value) = inherited.get(*attr_name) {
                                log::debug!(
                                    "Page {} inheriting /{} from ancestor Pages node",
                                    target_index,
                                    attr_name
                                );
                                page_dict.insert(attr_name.to_string(), inherited_value.clone());
                            }
                        }
                    }

                    Ok(Object::Dictionary(page_dict))
                } else {
                    *current_index += 1;
                    Err(Error::InvalidPdf(format!("Page index {} not found in tree", target_index)))
                }
            },
            "Pages" => {
                // This is an intermediate Pages node with kids
                // Collect inheritable attributes from this node to pass to children
                let inheritable_attrs = ["Resources", "MediaBox", "CropBox", "Rotate"];

                for attr_name in &inheritable_attrs {
                    if let Some(attr_value) = node_dict.get(*attr_name) {
                        // Only add if not already in inherited map (child values override parent)
                        inherited
                            .entry(attr_name.to_string())
                            .or_insert_with(|| attr_value.clone());
                    }
                }

                // Try to get /Kids array; if missing, this is a malformed PDF
                let kids = match node_dict.get("Kids").and_then(|obj| obj.as_array()) {
                    Some(k) => k,
                    None => {
                        log::warn!("Malformed PDF: Pages node missing /Kids array");
                        // Malformed PDF: Pages node has no /Kids array
                        // Gracefully return without error to allow other recovery paths
                        // The scanning method will find pages eventually
                        return Err(Error::InvalidPdf(
                            "Pages node missing /Kids array - try fallback method".to_string(),
                        ));
                    },
                };

                for kid in kids {
                    let kid_ref = kid.as_reference().ok_or_else(|| {
                        Error::InvalidPdf("Kid in /Kids array is not a reference".to_string())
                    })?;

                    // Pass inherited attributes to children
                    match self.get_page_from_tree_inner(
                        kid_ref,
                        target_index,
                        current_index,
                        inherited,
                        visited,
                    ) {
                        Ok(page) => return Ok(page),
                        Err(Error::CircularReference(obj_ref)) => {
                            log::warn!(
                                "Circular reference in page tree at object {}, skipping",
                                obj_ref
                            );
                            continue;
                        },
                        Err(Error::RecursionLimitExceeded(_)) => {
                            log::warn!("Recursion limit exceeded in page tree, skipping branch");
                            continue;
                        },
                        Err(_) => continue,
                    }
                }

                Err(Error::InvalidPdf(format!("Page index {} not found", target_index)))
            },
            _ => Err(Error::InvalidPdf(format!("Unknown page tree node type: {}", node_type))),
        }
    }

    /// Get the object reference for a page by index.
    ///
    /// This is used by outline and annotations to find page references.
    pub(crate) fn get_page_ref(&mut self, page_index: usize) -> Result<ObjectRef> {
        let catalog = self.catalog()?;
        let catalog_dict = catalog.as_dict().ok_or_else(|| Error::InvalidObjectType {
            expected: "Dictionary".to_string(),
            found: "Other".to_string(),
        })?;

        let pages_ref = catalog_dict
            .get("Pages")
            .ok_or_else(|| Error::InvalidPdf("Catalog missing /Pages entry".to_string()))?
            .as_reference()
            .ok_or_else(|| Error::InvalidPdf("/Pages is not a reference".to_string()))?;

        self.get_page_ref_recursive(pages_ref, page_index, &mut 0, &mut HashSet::new())
    }

    /// Recursively find page reference in the page tree.
    pub(crate) fn get_page_ref_recursive(
        &mut self,
        node_ref: ObjectRef,
        target_index: usize,
        current_index: &mut usize,
        visited: &mut HashSet<ObjectRef>,
    ) -> Result<ObjectRef> {
        if !visited.insert(node_ref) {
            return Err(Error::CircularReference(node_ref));
        }
        let node = self.load_object(node_ref)?;
        let node_dict = node.as_dict().ok_or_else(|| Error::InvalidObjectType {
            expected: "Dictionary".to_string(),
            found: "Other".to_string(),
        })?;

        let node_type = node_dict
            .get("Type")
            .and_then(|t| t.as_name())
            .ok_or_else(|| Error::InvalidPdf("Node missing Type".to_string()))?;

        match node_type {
            "Page" => {
                if *current_index == target_index {
                    Ok(node_ref)
                } else {
                    *current_index += 1;
                    Err(Error::InvalidPdf(format!("Page {} not found", target_index)))
                }
            },
            "Pages" => {
                let kids = node_dict
                    .get("Kids")
                    .and_then(|k| k.as_array())
                    .ok_or_else(|| Error::InvalidPdf("Pages node missing Kids".to_string()))?;

                for kid_obj in kids {
                    if let Some(kid_ref) = kid_obj.as_reference() {
                        match self.get_page_ref_recursive(
                            kid_ref,
                            target_index,
                            current_index,
                            visited,
                        ) {
                            Ok(page_ref) => return Ok(page_ref),
                            Err(_) => continue,
                        }
                    }
                }

                Err(Error::InvalidPdf(format!("Page {} not found", target_index)))
            },
            _ => Err(Error::InvalidPdf(format!("Unknown node type: {}", node_type))),
        }
    }

    /// Extract text from a page as a plain string.
    ///
    /// # Arguments
    ///
    /// * `page_index` - Zero-based page index
    ///
    /// # Returns
    ///
    /// The extracted text as a string.
    ///
    /// # Errors
    ///
    /// Returns an error if the page cannot be accessed or text extraction fails.
    /// Decode PDF escape sequences in text (e.g., \274 -> §, \( -> (, etc.)
    #[allow(dead_code)]
    fn decode_pdf_escapes(text: &str) -> String {
        let mut result = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '\\' {
                // Check what follows the backslash
                match chars.peek() {
                    Some(&'(') => {
                        result.push('(');
                        chars.next();
                    },
                    Some(&')') => {
                        result.push(')');
                        chars.next();
                    },
                    Some(&'\\') => {
                        result.push('\\');
                        chars.next();
                    },
                    Some(&'n') => {
                        result.push('\n');
                        chars.next();
                    },
                    Some(&'r') => {
                        result.push('\r');
                        chars.next();
                    },
                    Some(&'t') => {
                        result.push('\t');
                        chars.next();
                    },
                    Some(&'?') => {
                        // \? is a soft hyphen (optional line break point)
                        // Just skip it
                        chars.next();
                    },
                    Some(d) if d.is_ascii_digit() => {
                        // Octal escape sequence: \ddd
                        let mut octal = String::new();
                        for _ in 0..3 {
                            if let Some(&digit) = chars.peek() {
                                if digit.is_ascii_digit() && digit < '8' {
                                    octal.push(digit);
                                    chars.next();
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }

                        if !octal.is_empty() {
                            if let Ok(code) = u8::from_str_radix(&octal, 8) {
                                // PDFDocEncoding: ISO 32000-1:2008, Annex D
                                let decoded_char = Self::pdfdoc_decode(code);
                                result.push(decoded_char);
                            } else {
                                // Failed to parse, keep the backslash and octal
                                result.push('\\');
                                result.push_str(&octal);
                            }
                        } else {
                            // No valid octal digits, keep the backslash
                            result.push('\\');
                        }
                    },
                    _ => {
                        // Unknown escape, keep the backslash
                        result.push('\\');
                    },
                }
            } else {
                result.push(ch);
            }
        }

        result
    }

    /// Decode a byte using PDFDocEncoding (ISO 32000-1:2008, Annex D).
    ///
    /// PDFDocEncoding is the default encoding for text strings in PDF:
    /// - Codes 0-127: ASCII
    /// - Codes 128-159: Special Unicode characters
    /// - Codes 160-255: Latin-1 (ISO 8859-1)
    #[allow(dead_code)]
    fn pdfdoc_decode(code: u8) -> char {
        match code {
            // 0-127: Standard ASCII
            0..=127 => code as char,

            // 128-159: PDFDocEncoding special mappings
            128 => '\u{2022}', // BULLET
            129 => '\u{2020}', // DAGGER
            130 => '\u{2021}', // DOUBLE DAGGER
            131 => '\u{2026}', // HORIZONTAL ELLIPSIS
            132 => '\u{2014}', // EM DASH
            133 => '\u{2013}', // EN DASH
            134 => '\u{0192}', // LATIN SMALL LETTER F WITH HOOK
            135 => '\u{2044}', // FRACTION SLASH
            136 => '\u{2039}', // SINGLE LEFT-POINTING ANGLE QUOTATION MARK
            137 => '\u{203A}', // SINGLE RIGHT-POINTING ANGLE QUOTATION MARK
            138 => '\u{2212}', // MINUS SIGN
            139 => '\u{2030}', // PER MILLE SIGN
            140 => '\u{201E}', // DOUBLE LOW-9 QUOTATION MARK
            141 => '\u{201C}', // LEFT DOUBLE QUOTATION MARK
            142 => '\u{201D}', // RIGHT DOUBLE QUOTATION MARK
            143 => '\u{2018}', // LEFT SINGLE QUOTATION MARK
            144 => '\u{2019}', // RIGHT SINGLE QUOTATION MARK
            145 => '\u{201A}', // SINGLE LOW-9 QUOTATION MARK
            146 => '\u{2122}', // TRADE MARK SIGN
            147 => '\u{FB01}', // LATIN SMALL LIGATURE FI
            148 => '\u{FB02}', // LATIN SMALL LIGATURE FL
            149 => '\u{0141}', // LATIN CAPITAL LETTER L WITH STROKE
            150 => '\u{0152}', // LATIN CAPITAL LIGATURE OE
            151 => '\u{0160}', // LATIN CAPITAL LETTER S WITH CARON
            152 => '\u{0178}', // LATIN CAPITAL LETTER Y WITH DIAERESIS
            153 => '\u{017D}', // LATIN CAPITAL LETTER Z WITH CARON
            154 => '\u{0131}', // LATIN SMALL LETTER DOTLESS I
            155 => '\u{0142}', // LATIN SMALL LETTER L WITH STROKE
            156 => '\u{0153}', // LATIN SMALL LIGATURE OE
            157 => '\u{0161}', // LATIN SMALL LETTER S WITH CARON
            158 => '\u{017E}', // LATIN SMALL LETTER Z WITH CARON
            159 => '\u{FFFD}', // REPLACEMENT CHARACTER (undefined in PDFDocEncoding)

            // 160-255: Latin-1 (ISO 8859-1)
            160..=255 => code as char,
        }
    }

    /// Circular references and recursion limit errors are handled gracefully
    /// with warning messages in the output.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # let mut doc = PdfDocument::open("sample.pdf")?;
    /// let text = doc.extract_text(0)?;
    /// println!("Page 1 text: {}", text);
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn extract_text(&mut self, page_index: usize) -> Result<String> {
        // PDF Spec ISO 32000-1:2008 Section 14.8.2.3:
        // For Tagged PDFs, use structure tree for reading order (spec-compliant)
        // For Untagged PDFs, use page content order (spec-compliant)

        // Check if this is a Tagged PDF with structure tree
        if let Ok(Some(struct_tree)) = self.structure_tree() {
            // Tagged PDF: Use structure tree for correct reading order
            log::debug!(
                "Using structure tree for Tagged PDF text extraction (page {})",
                page_index
            );
            return self.extract_text_structure_order(page_index, &struct_tree);
        }

        // Untagged PDF: Use page content order (current implementation)
        log::debug!(
            "Using page content order for Untagged PDF text extraction (page {})",
            page_index
        );

        // Use PDF spec-compliant TextSpan extraction (RECOMMENDED approach)
        // This preserves the PDF's text positioning intent and avoids overlapping character issues
        let spans = self.extract_spans(page_index)?;

        // OCR fallback for scanned PDFs (when OCR feature is enabled)
        // If no text spans found, check if page needs OCR
        #[cfg(feature = "ocr")]
        if spans.is_empty() || spans.iter().map(|s| s.text.len()).sum::<usize>() < 50 {
            // Check if this looks like a scanned page
            if let Ok(true) = crate::ocr::needs_ocr(self, page_index) {
                log::debug!(
                    "Page {} appears to be scanned, OCR available but not auto-enabled",
                    page_index
                );
                // Note: We don't automatically run OCR here because:
                // 1. It requires model files that may not be available
                // 2. Users should opt-in via extract_text_with_ocr or similar
                // 3. This keeps extract_text fast and predictable
            }
        }

        if spans.is_empty() {
            return Ok(String::new());
        }

        // Assemble text from spans, preserving reading order
        let mut text = String::with_capacity(spans.len() * 20); // estimate
        let mut prev_span: Option<&TextSpan> = None;

        for span in &spans {
            // Check if we need to insert space or line break
            if let Some(prev) = prev_span {
                let y_diff = (prev.bbox.y - span.bbox.y).abs();

                // New line if Y position changed significantly (more than 2pt)
                if y_diff > 2.0 {
                    // Calculate number of line breaks based on Y gap
                    let font_size = span.font_size.max(10.0);
                    let line_height = font_size * 1.2; // typical line height
                    let num_breaks = (y_diff / line_height).round() as usize;

                    // Add line breaks (at least 1, max 3 for large gaps)
                    for _ in 0..num_breaks.clamp(1, 3) {
                        text.push('\n');
                    }
                } else if Self::should_insert_space(prev, span) {
                    // Same line but significant horizontal gap - insert space
                    // This handles PDFs that don't include space characters (ISO 32000-1:2008 Section 9.3.3)
                    text.push(' ');
                }
            }

            text.push_str(&span.text);
            prev_span = Some(span);
        }

        // Apply whitespace cleanup for better readability
        // This normalizes excessive double spaces and blank lines
        let cleaned_text = crate::converters::whitespace::cleanup_plain_text(&text);

        Ok(cleaned_text)
    }

    /// Extract text from a page with automatic OCR fallback for scanned pages.
    ///
    /// This method automatically detects scanned pages and applies OCR when needed,
    /// falling back to native text extraction for regular PDFs.
    ///
    /// **Note**: Requires the `ocr` feature to be enabled and OCR models to be provided.
    ///
    /// # Arguments
    ///
    /// * `page_index` - Page number (0-indexed)
    /// * `ocr_engine` - Optional OCR engine (required for scanned pages)
    /// * `ocr_options` - OCR extraction options (DPI, thresholds, etc.)
    ///
    /// # Returns
    ///
    /// The extracted text, either from native PDF text or OCR.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use pdf_oxide::{PdfDocument, ocr::{OcrEngine, OcrConfig, OcrExtractOptions}};
    ///
    /// let mut doc = PdfDocument::open("mixed.pdf")?;
    /// let engine = OcrEngine::new("det.onnx", "rec.onnx", "dict.txt", OcrConfig::default())?;
    ///
    /// // Automatically uses native text or OCR as needed
    /// let text = doc.extract_text_with_ocr(0, Some(&engine), OcrExtractOptions::default())?;
    /// ```
    #[cfg(feature = "ocr")]
    pub fn extract_text_with_ocr(
        &mut self,
        page_index: usize,
        ocr_engine: Option<&crate::ocr::OcrEngine>,
        ocr_options: crate::ocr::OcrExtractOptions,
    ) -> Result<String> {
        crate::ocr::extract_text_with_ocr(self, page_index, ocr_engine, ocr_options)
    }

    /// Extract TextSpans with automatic OCR fallback for scanned pages.
    ///
    /// This method extracts text spans using native PDF text extraction, but falls back
    /// to OCR when the page appears to be scanned (no/minimal native text).
    ///
    /// **Note**: Requires the `ocr` feature to be enabled and OCR models to be provided.
    ///
    /// # Arguments
    ///
    /// * `page_index` - Page number (0-indexed)
    /// * `ocr_engine` - Optional OCR engine (required for scanned pages)
    /// * `ocr_options` - OCR extraction options (DPI, thresholds, etc.)
    ///
    /// # Returns
    ///
    /// Vector of TextSpans, either from native PDF or OCR.
    #[cfg(feature = "ocr")]
    pub fn extract_spans_with_ocr(
        &mut self,
        page_index: usize,
        ocr_engine: Option<&crate::ocr::OcrEngine>,
        ocr_options: &crate::ocr::OcrExtractOptions,
    ) -> Result<Vec<crate::layout::TextSpan>> {
        // First try native text extraction
        let spans = self.extract_spans(page_index)?;

        // If we got substantial text, return it
        if !spans.is_empty() && spans.iter().map(|s| s.text.len()).sum::<usize>() >= 50 {
            return Ok(spans);
        }

        // Check if page needs OCR
        if let Ok(true) = crate::ocr::needs_ocr(self, page_index) {
            // Try OCR if engine is available
            if let Some(engine) = ocr_engine {
                match crate::ocr::ocr_page_spans(self, page_index, engine, ocr_options) {
                    Ok(ocr_spans) if !ocr_spans.is_empty() => return Ok(ocr_spans),
                    Ok(_) => log::debug!("OCR returned no spans for page {}", page_index),
                    Err(e) => log::warn!("OCR failed for page {}: {}", page_index, e),
                }
            }
        }

        // Fallback to native spans (even if empty)
        Ok(spans)
    }

    /// Determine if a space should be inserted between two text spans.
    ///
    /// According to PDF spec (ISO 32000-1:2008 Section 9.3.3), word spacing
    /// only applies to actual space characters (0x20). Many PDFs (especially
    /// academic papers) use precise positioning instead of space characters.
    /// This function detects such gaps and inserts spaces heuristically.
    ///
    /// # Algorithm
    /// 1. Check if spans are on the same line (Y positions similar)
    /// 2. Calculate horizontal gap between end of prev span and start of current span
    /// 3. Insert space if gap exceeds threshold (0.25 × font size)
    ///
    /// # Arguments
    /// * `prev` - Previous text span
    /// * `current` - Current text span
    ///
    /// # Returns
    /// `true` if a space should be inserted between the spans
    fn should_insert_space(prev: &TextSpan, current: &TextSpan) -> bool {
        // Get font size (use the larger of the two)
        let font_size = prev.font_size.max(current.font_size).max(1.0);

        // Check if spans are on the same line
        // Y difference should be small (< 30% of font size)
        let y_diff = (prev.bbox.y - current.bbox.y).abs();
        if y_diff > font_size * 0.3 {
            return false; // Different lines - no space needed
        }

        // Calculate horizontal gap
        let prev_end_x = prev.bbox.x + prev.bbox.width;
        let gap = current.bbox.x - prev_end_x;

        // Space threshold: 0.25 × font size (quarter of font size)
        // This is based on testing with PyMuPDF4LLM and empirical observation
        let space_threshold = font_size * 0.25;

        // Insert space if gap is significant
        // Also check that gap is not too large (might indicate column boundary)
        gap > space_threshold && gap < font_size * 5.0
    }

    /// Extract text using structure tree for Tagged PDFs.
    ///
    /// This method implements PDF spec-compliant text extraction for Tagged PDFs
    /// using the logical structure tree to determine reading order.
    ///
    /// # PDF Spec Reference
    ///
    /// ISO 32000-1:2008 Section 14.8.2.3 - Determining the Text Extraction Sequence
    /// "For a Tagged PDF document, conforming readers shall present the document's
    /// content to the user in the order given by a pre-order traversal of the
    /// structure hierarchy"
    ///
    /// # Algorithm
    /// 1. Extract all text spans with MCIDs from the page
    /// 2. Build a map from MCID → Vec<TextSpan>
    /// 3. Traverse structure tree in pre-order to get MCIDs in reading order
    /// 4. Assemble text by looking up spans for each MCID in order
    ///
    /// # Arguments
    /// * `page_index` - Zero-based page index
    /// * `struct_tree` - The structure tree root from the PDF catalog
    ///
    /// # Returns
    /// Extracted text in logical structure order
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // This is called automatically by extract_text() for Tagged PDFs
    /// let text = doc.extract_text(0)?;
    /// ```
    fn extract_text_structure_order(
        &mut self,
        page_index: usize,
        struct_tree: &crate::structure::StructTreeRoot,
    ) -> Result<String> {
        log::debug!("Extracting text using structure tree for page {}", page_index);

        // Step 1: Extract all spans with MCIDs
        let all_spans = self.extract_spans(page_index)?;

        if all_spans.is_empty() {
            return Ok(String::new());
        }

        // Step 2: Build MCID → Vec<TextSpan> map
        let mut mcid_map: HashMap<u32, Vec<TextSpan>> = HashMap::new();
        let mut spans_without_mcid: Vec<TextSpan> = Vec::new();

        for span in all_spans {
            if let Some(mcid) = span.mcid {
                mcid_map.entry(mcid).or_default().push(span);
            } else {
                // Collect spans without MCID (shouldn't happen in well-formed Tagged PDFs)
                spans_without_mcid.push(span);
            }
        }

        log::debug!(
            "Found {} MCIDs with spans, {} spans without MCID",
            mcid_map.len(),
            spans_without_mcid.len()
        );

        // Step 3: Traverse structure tree to get MCIDs in reading order
        let ordered_content = traverse_structure_tree(struct_tree, page_index as u32)
            .map_err(|e| Error::InvalidPdf(format!("Failed to traverse structure tree: {}", e)))?;

        log::debug!(
            "Structure tree traversal found {} content items in reading order",
            ordered_content.len()
        );

        // Step 4: Assemble text in structure order
        let mut text = String::with_capacity(mcid_map.len() * 50); // estimate
        let mut prev_span: Option<&TextSpan> = None;

        for content in &ordered_content {
            // Handle word break markers by inserting a space
            if content.is_word_break {
                if !text.is_empty() && !text.ends_with(' ') && !text.ends_with('\n') {
                    text.push(' ');
                }
                continue;
            }

            // For regular content with MCID
            let Some(mcid) = content.mcid else {
                continue; // Skip entries without MCID (shouldn't happen except for WB)
            };

            if let Some(spans) = mcid_map.get(&mcid) {
                // Process all spans for this MCID
                for span in spans {
                    // Check if we need space or line break
                    if let Some(prev) = prev_span {
                        let y_diff = (prev.bbox.y - span.bbox.y).abs();

                        if y_diff > 2.0 {
                            // New line
                            let font_size = span.font_size.max(10.0);
                            let line_height = font_size * 1.2;
                            let num_breaks = (y_diff / line_height).round() as usize;
                            for _ in 0..num_breaks.clamp(1, 3) {
                                text.push('\n');
                            }
                        } else if Self::should_insert_space(prev, span) {
                            text.push(' ');
                        }
                    }

                    text.push_str(&span.text);
                    prev_span = Some(span);
                }
            } else {
                log::warn!(
                    "Structure tree references MCID {} but no spans found with that MCID",
                    mcid
                );
            }
        }

        // Append any spans without MCID at the end (shouldn't happen in well-formed PDFs)
        if !spans_without_mcid.is_empty() {
            log::warn!(
                "Found {} text spans without MCID - appending to end",
                spans_without_mcid.len()
            );
            for span in &spans_without_mcid {
                if let Some(prev) = prev_span {
                    let y_diff = (prev.bbox.y - span.bbox.y).abs();
                    if y_diff > 2.0 {
                        text.push('\n');
                    } else if Self::should_insert_space(prev, span) {
                        text.push(' ');
                    }
                }
                text.push_str(&span.text);
                prev_span = Some(span);
            }
        }

        Ok(text)
    }

    /// Extract text spans from a page (PDF spec compliant - RECOMMENDED).
    ///
    /// This is the recommended method for text extraction. It extracts complete
    /// text strings as the PDF provides them via Tj/TJ operators, following the
    /// PDF specification ISO 32000-1:2008.
    ///
    /// # Benefits over extract_chars
    /// - Avoids overlapping character issues
    /// - Preserves PDF's text positioning intent
    /// - More robust for complex layouts
    /// - Matches industry best practices (PyMuPDF, etc.)
    ///
    /// # Arguments
    ///
    /// * `page_index` - Zero-based page index
    ///
    /// # Returns
    ///
    /// Vector of TextSpan objects in reading order
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use pdf_oxide::PdfDocument;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("document.pdf")?;
    /// let spans = doc.extract_spans(0)?;
    /// for span in spans {
    ///     println!("Text: {} at ({}, {})", span.text, span.bbox.x, span.bbox.y);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn extract_spans(&mut self, page_index: usize) -> Result<Vec<crate::layout::TextSpan>> {
        use crate::extractors::{TextExtractionConfig, TextExtractor};
        use crate::text::document_classifier::DocumentClassifier;

        // Get page object
        let page = self.get_page(page_index)?;
        let page_dict = page.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "Page is not a dictionary".to_string(),
        })?;

        // Get content stream data (reuse the same logic as extract_chars)
        let content_data = self.get_page_content_data(page_index)?;

        // First pass: Extract with conservative thresholds to analyze document
        let mut initial_extractor = TextExtractor::new();
        if let Some(resources) = page_dict.get("Resources") {
            initial_extractor.set_resources(resources.clone());
            initial_extractor.set_document(self as *mut PdfDocument);
            self.load_fonts(resources, &mut initial_extractor)?;
        }
        let initial_spans = initial_extractor.extract_text_spans(&content_data)?;

        // Classify document type based on extracted content
        // Convert TextSpans to text lines for classification
        let text_lines: Vec<&str> = initial_spans
            .iter()
            .filter_map(|span| {
                // Skip empty spans
                if span.text.trim().is_empty() {
                    None
                } else {
                    Some(span.text.as_str())
                }
            })
            .collect();

        let (doc_type, _stats) = DocumentClassifier::classify_lines(text_lines.into_iter());

        // Select appropriate profile for this document type
        let profile = crate::config::ExtractionProfile::for_document_type(doc_type);

        // Create configured text extractor with profile-specific thresholds
        let config = TextExtractionConfig::default().with_profile(profile);
        let mut final_extractor = TextExtractor::with_config(config);

        // Load fonts from page resources and set resources for XObject access
        if let Some(resources) = page_dict.get("Resources") {
            final_extractor.set_resources(resources.clone());
            final_extractor.set_document(self as *mut PdfDocument);

            // Load fonts
            self.load_fonts(resources, &mut final_extractor)?;
        }

        // Extract text spans with profile-optimized thresholds
        final_extractor.extract_text_spans(&content_data)
    }

    /// Extract text spans from a page with custom configuration.
    ///
    /// This method allows controlling span merging behavior through configuration,
    /// including adaptive threshold settings for improved extraction quality.
    ///
    /// # Arguments
    ///
    /// * `page_index` - Zero-based page index
    /// * `config` - SpanMergingConfig controlling extraction parameters
    ///
    /// # Returns
    ///
    /// A vector of TextSpan objects extracted from the page with applied configuration.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # use pdf_oxide::extractors::SpanMergingConfig;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("example.pdf")?;
    ///
    /// // Use adaptive threshold configuration
    /// let config = SpanMergingConfig::adaptive();
    /// let spans = doc.extract_spans_with_config(0, config)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn extract_spans_with_config(
        &mut self,
        page_index: usize,
        config: crate::extractors::SpanMergingConfig,
    ) -> Result<Vec<crate::layout::TextSpan>> {
        use crate::extractors::TextExtractor;

        // Get page object
        let page = self.get_page(page_index)?;
        let page_dict = page.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "Page is not a dictionary".to_string(),
        })?;

        // Get content stream data
        let content_data = self.get_page_content_data(page_index)?;

        // Create text extractor with merged configuration
        let mut extractor = TextExtractor::new().with_merging_config(config);

        // Load fonts from page resources and set resources for XObject access
        if let Some(resources) = page_dict.get("Resources") {
            extractor.set_resources(resources.clone());
            extractor.set_document(self as *mut PdfDocument);

            // Load fonts
            self.load_fonts(resources, &mut extractor)?;
        }

        // Extract text spans
        extractor.extract_text_spans(&content_data)
    }

    /// Extract individual characters from a PDF page.
    ///
    /// This is a **low-level API** for character-level granularity. For most use cases,
    /// prefer `extract_spans()` which provides complete text strings as PDF defines them.
    ///
    /// # Character-level extraction details:
    ///
    /// - Returns individual `TextChar` objects with position, font, and style information
    /// - Characters are sorted in reading order (top-to-bottom, left-to-right)
    /// - Overlapping characters (rendered multiple times for effects) are deduplicated
    /// - Useful for layout analysis, debugging, or custom text processing pipelines
    ///
    /// # Arguments
    ///
    /// * `page_index` - Page number (0-indexed)
    ///
    /// # Returns
    ///
    /// Vector of `TextChar` objects in reading order, or error if extraction fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("document.pdf")?;
    /// let chars = doc.extract_chars(0)?;
    /// for ch in chars {
    ///     println!("'{}' at ({:.1}, {:.1}), font: {}",
    ///         ch.char, ch.bbox.x, ch.bbox.y, ch.font_name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Performance Note
    ///
    /// Character extraction is typically 30-50% faster than span extraction
    /// because it skips the text grouping and merging logic.
    pub fn extract_chars(&mut self, page_index: usize) -> Result<Vec<crate::layout::TextChar>> {
        use crate::extractors::TextExtractor;

        // Get page object
        let page = self.get_page(page_index)?;
        let page_dict = page.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "Page is not a dictionary".to_string(),
        })?;

        // Get content stream data
        let content_data = self.get_page_content_data(page_index)?;

        // Create text extractor for character-level extraction
        let mut extractor = TextExtractor::new();

        // Load fonts from page resources and set resources for XObject access
        if let Some(resources) = page_dict.get("Resources") {
            extractor.set_resources(resources.clone());
            extractor.set_document(self as *mut PdfDocument);

            // Load fonts
            self.load_fonts(resources, &mut extractor)?;
        }

        // Extract characters directly (single-pass, no document classification)
        extractor.extract(&content_data)
    }

    /// Apply intelligent text post-processing to extracted text spans.
    ///
    /// This method applies several text quality improvements:
    /// - Ligature expansion (fi, fl, ffi, ffl → component characters)
    /// - Hyphenation reconstruction (rejoins words split across lines)
    /// - Whitespace normalization (removes excess spaces within words)
    /// - Special character spacing (Greek letters, math symbols)
    /// - OCR text cleanup (when font_name == "OCR" or from known OCR engines)
    ///
    /// # Arguments
    ///
    /// * `spans` - Vector of TextSpan extracted from pages
    ///
    /// # Returns
    ///
    /// Processed spans with improved text quality
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use pdf_oxide::PdfDocument;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("example.pdf")?;
    ///
    /// // Extract spans from page
    /// let spans = doc.extract_spans(0)?;
    ///
    /// // Apply intelligent processing
    /// let processed = doc.apply_intelligent_text_processing(spans);
    ///
    /// for span in &processed {
    ///     println!("{}", span.text); // Ligatures expanded, hyphenation fixed
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn apply_intelligent_text_processing(&self, mut spans: Vec<TextSpan>) -> Vec<TextSpan> {
        use crate::converters::text_post_processor::TextPostProcessor;
        use crate::text::ligature_processor::get_ligature_components;

        for span in &mut spans {
            // Step 1: Detect if this is OCR text (from our OCR or known OCR engines)
            let is_ocr = span.font_name == "OCR"
                || span.font_name.to_lowercase().contains("tesseract")
                || span.font_name.to_lowercase().contains("abbyy");

            // Step 2: Expand ligatures in text
            let mut expanded = String::with_capacity(span.text.len() * 2);
            for ch in span.text.chars() {
                if let Some(components) = get_ligature_components(ch) {
                    expanded.push_str(components);
                } else {
                    expanded.push(ch);
                }
            }

            // Step 3: Apply text post-processing pipeline
            // (hyphenation, whitespace, special char spacing)
            span.text = TextPostProcessor::process(&expanded);

            // Step 4: Additional OCR-specific cleanup if needed
            if is_ocr {
                // OCR text often has extra artifacts - do additional cleanup
                span.text = span
                    .text
                    .replace("ﬁ", "fi") // Sometimes OCR keeps ligatures
                    .replace("ﬂ", "fl")
                    .replace("ﬀ", "ff")
                    .replace("  ", " "); // Double space cleanup
            }
        }

        spans
    }

    /// Extract hierarchical content structure from a page.
    ///
    /// Returns the page's hierarchical content structure with all children populated.
    /// For tagged PDFs with structure trees, returns the structure with extracted content.
    /// For untagged PDFs, returns a synthetic hierarchy based on geometric analysis.
    ///
    /// # Arguments
    ///
    /// * `page_index` - The page to extract from (0-indexed)
    ///
    /// # Returns
    ///
    /// `Ok(Some(structure))` if structure is found or generated,
    /// `Ok(None)` if no structure is available,
    /// `Err` if an error occurs during extraction
    ///
    /// # PDF Spec Compliance
    ///
    /// - ISO 32000-1:2008, Section 14.7 - Logical Structure
    /// - ISO 32000-1:2008, Section 14.8 - Tagged PDF
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("example.pdf")?;
    ///
    /// // Extract hierarchical structure from first page
    /// if let Some(structure) = doc.extract_hierarchical_content(0)? {
    ///     println!("Document structure type: {}", structure.structure_type);
    ///     println!("Number of children: {}", structure.children.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn extract_hierarchical_content(
        &mut self,
        page_index: usize,
    ) -> Result<Option<crate::elements::StructureElement>> {
        use crate::extractors::HierarchicalExtractor;
        HierarchicalExtractor::extract_page(self, page_index)
    }

    /// Get the raw content stream data for a page.
    ///
    /// This returns the decoded content stream bytes for the specified page.
    /// The content stream contains PDF operators that define the page's appearance.
    pub fn get_page_content_data(&mut self, page_index: usize) -> Result<Vec<u8>> {
        // Ensure encryption is initialized if needed
        self.ensure_encryption_initialized()?;

        // Get page object
        let page = self.get_page(page_index)?;
        let page_dict = page.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "Page is not a dictionary".to_string(),
        })?;

        // Get content stream(s) — Contents is optional per ISO 32000-1:2008 Table 30
        let contents_ref = match page_dict.get("Contents") {
            Some(Object::Null) | None => {
                log::debug!("Page {} has no /Contents (blank page)", page_index);
                return Ok(Vec::new());
            },
            Some(c) => c,
        };

        // Contents can be either a single stream, an array of streams, or a direct stream object
        let content_data = if let Some(contents_ref_val) = contents_ref.as_reference() {
            // Contents is a reference - it could point to either a Stream or an Array
            let contents = self.load_object(contents_ref_val)?;

            // Check if the loaded object is an Array (indirect array)
            if let Some(contents_array) = contents.as_array() {
                // The reference pointed to an array of streams
                let mut combined = Vec::new();

                for content_item in contents_array.iter() {
                    if matches!(content_item, Object::Null) {
                        continue;
                    }
                    if let Some(ref_val) = content_item.as_reference() {
                        let content_obj = self.load_object(ref_val)?;
                        let decoded = self.decode_stream_with_encryption(&content_obj, ref_val)?;
                        combined.extend_from_slice(&decoded);
                        combined.push(b'\n');
                    } else {
                        let decoded = content_item.decode_stream_data()?;
                        combined.extend_from_slice(&decoded);
                        combined.push(b'\n');
                    }
                }

                combined
            } else {
                // The reference pointed to a single stream
                // Decode with encryption support, using the object reference
                self.decode_stream_with_encryption(&contents, contents_ref_val)?
            }
        } else if let Some(contents_array) = contents_ref.as_array() {
            // Array of streams - can be references or direct objects
            let mut combined = Vec::new();

            for content_item in contents_array.iter() {
                if matches!(content_item, Object::Null) {
                    continue;
                }
                if let Some(ref_val) = content_item.as_reference() {
                    let content_obj = self.load_object(ref_val)?;
                    let decoded = self.decode_stream_with_encryption(&content_obj, ref_val)?;
                    combined.extend_from_slice(&decoded);
                    combined.push(b'\n');
                } else {
                    let decoded = content_item.decode_stream_data()?;
                    combined.extend_from_slice(&decoded);
                    combined.push(b'\n');
                }
            }

            combined
        } else {
            // Direct stream object (rare but possible)
            // For direct objects, use regular decoding (no encryption key)
            contents_ref.decode_stream_data()?
        };

        Ok(content_data)
    }

    /// Extract path (vector graphics) content from a page.
    ///
    /// This extracts all vector graphics operations from the page's content stream,
    /// including lines, curves, rectangles, and shapes.
    ///
    /// # Arguments
    ///
    /// * `page_index` - Zero-based page index
    ///
    /// # Returns
    ///
    /// A vector of `PathContent` objects representing all paths on the page.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("example.pdf")?;
    ///
    /// // Extract paths from first page
    /// let paths = doc.extract_paths(0)?;
    ///
    /// for path in paths {
    ///     println!("Path with {} operations, bbox: {:?}",
    ///         path.operations.len(), path.bbox);
    ///     if path.has_stroke() {
    ///         println!("  Stroked with width: {}", path.stroke_width);
    ///     }
    ///     if path.has_fill() {
    ///         println!("  Filled");
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn extract_paths(
        &mut self,
        page_index: usize,
    ) -> Result<Vec<crate::elements::PathContent>> {
        use crate::content::{parse_content_stream, GraphicsStateStack, Operator};
        use crate::elements::{LineCap, LineJoin};
        use crate::extractors::paths::{FillRule, PathExtractor};
        use crate::layout::Color;

        // Get page object and content stream
        let page = self.get_page(page_index)?;
        let page_dict = page.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "Page is not a dictionary".to_string(),
        })?;
        let content_data = self.get_page_content_data(page_index)?;

        // Parse content stream into operators
        let operators = parse_content_stream(&content_data)?;

        // Create path extractor and graphics state stack
        let mut extractor = PathExtractor::new();
        let mut state_stack = GraphicsStateStack::new();

        // Resolve and set page resources for XObject processing
        if let Some(resources) = page_dict.get("Resources") {
            let resolved_resources = if let Some(ref_obj) = resources.as_reference() {
                self.load_object(ref_obj)?
            } else {
                resources.clone()
            };
            extractor.set_resources(resolved_resources);
        }

        // Process each operator
        for op in operators {
            match op {
                // Graphics state operators
                Operator::SaveState => {
                    state_stack.save();
                },
                Operator::RestoreState => {
                    state_stack.restore();
                    extractor.update_from_state(state_stack.current());
                },
                Operator::Cm { a, b, c, d, e, f } => {
                    let state = state_stack.current_mut();
                    let new_matrix = crate::content::Matrix { a, b, c, d, e, f };
                    state.ctm = state.ctm.multiply(&new_matrix);
                    extractor.set_ctm(state.ctm);
                },

                // Color operators (stroke)
                Operator::SetStrokeRgb { r, g, b } => {
                    state_stack.current_mut().stroke_color_rgb = (r, g, b);
                    extractor.set_stroke_color(Color::new(r, g, b));
                },
                Operator::SetStrokeGray { gray } => {
                    state_stack.current_mut().stroke_color_rgb = (gray, gray, gray);
                    extractor.set_stroke_color(Color::new(gray, gray, gray));
                },
                Operator::SetStrokeCmyk { c, m, y, k } => {
                    // Simple CMYK to RGB conversion
                    let r = (1.0 - c) * (1.0 - k);
                    let g = (1.0 - m) * (1.0 - k);
                    let b = (1.0 - y) * (1.0 - k);
                    state_stack.current_mut().stroke_color_rgb = (r, g, b);
                    extractor.set_stroke_color(Color::new(r, g, b));
                },

                // Color operators (fill)
                Operator::SetFillRgb { r, g, b } => {
                    state_stack.current_mut().fill_color_rgb = (r, g, b);
                    extractor.set_fill_color(Color::new(r, g, b));
                },
                Operator::SetFillGray { gray } => {
                    state_stack.current_mut().fill_color_rgb = (gray, gray, gray);
                    extractor.set_fill_color(Color::new(gray, gray, gray));
                },
                Operator::SetFillCmyk { c, m, y, k } => {
                    let r = (1.0 - c) * (1.0 - k);
                    let g = (1.0 - m) * (1.0 - k);
                    let b = (1.0 - y) * (1.0 - k);
                    state_stack.current_mut().fill_color_rgb = (r, g, b);
                    extractor.set_fill_color(Color::new(r, g, b));
                },

                // Line style operators
                Operator::SetLineWidth { width } => {
                    state_stack.current_mut().line_width = width;
                    extractor.set_line_width(width);
                },
                Operator::SetLineCap { cap_style } => {
                    state_stack.current_mut().line_cap = cap_style;
                    let cap = match cap_style {
                        1 => LineCap::Round,
                        2 => LineCap::Square,
                        _ => LineCap::Butt,
                    };
                    extractor.set_line_cap(cap);
                },
                Operator::SetLineJoin { join_style } => {
                    state_stack.current_mut().line_join = join_style;
                    let join = match join_style {
                        1 => LineJoin::Round,
                        2 => LineJoin::Bevel,
                        _ => LineJoin::Miter,
                    };
                    extractor.set_line_join(join);
                },

                // Path construction operators
                Operator::MoveTo { x, y } => {
                    extractor.move_to(x, y);
                },
                Operator::LineTo { x, y } => {
                    extractor.line_to(x, y);
                },
                Operator::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                } => {
                    extractor.curve_to(x1, y1, x2, y2, x3, y3);
                },
                Operator::CurveToV { x2, y2, x3, y3 } => {
                    extractor.curve_to_v(x2, y2, x3, y3);
                },
                Operator::CurveToY { x1, y1, x3, y3 } => {
                    extractor.curve_to_y(x1, y1, x3, y3);
                },
                Operator::Rectangle {
                    x,
                    y,
                    width,
                    height,
                } => {
                    extractor.rectangle(x, y, width, height);
                },
                Operator::ClosePath => {
                    extractor.close_path();
                },

                // Path painting operators
                Operator::Stroke => {
                    extractor.stroke();
                },
                Operator::Fill => {
                    extractor.fill(FillRule::NonZero);
                },
                Operator::FillEvenOdd => {
                    extractor.fill(FillRule::EvenOdd);
                },
                Operator::CloseFillStroke => {
                    extractor.close_fill_and_stroke(FillRule::NonZero);
                },
                Operator::EndPath => {
                    extractor.end_path();
                },

                // Clipping operators
                Operator::ClipNonZero => {
                    extractor.clip_non_zero();
                },
                Operator::ClipEvenOdd => {
                    extractor.clip_even_odd();
                },

                // XObject processing (Issue #40)
                Operator::Do { name } => {
                    if let Err(e) =
                        self.process_form_xobject_paths(&name, &mut extractor, &mut state_stack)
                    {
                        log::warn!(
                            "Failed to process XObject '{}' in path extraction: {}",
                            name,
                            e
                        );
                    }
                },

                // Skip other operators (text, images, etc.)
                _ => {},
            }
        }

        Ok(extractor.finish())
    }

    /// Process paths from a Form XObject (Issue #40).
    ///
    /// This method recursively extracts paths from Form XObjects encountered via the `Do` operator.
    /// It handles:
    /// - XObject resolution from resources
    /// - Type checking (Form vs Image)
    /// - Stream decoding and operator parsing
    /// - Coordinate transformations via /Matrix
    /// - Graphics state isolation
    ///
    /// # Arguments
    ///
    /// * `name` - The XObject name from the `Do` operator
    /// * `extractor` - The path extractor to accumulate paths
    /// * `state_stack` - The graphics state stack for transformations
    fn process_form_xobject_paths(
        &mut self,
        name: &str,
        extractor: &mut crate::extractors::paths::PathExtractor,
        state_stack: &mut crate::content::GraphicsStateStack,
    ) -> Result<()> {
        use crate::content::{parse_content_stream, Matrix, Operator};
        use crate::elements::{LineCap, LineJoin};
        use crate::extractors::paths::FillRule;
        use crate::layout::Color;

        // Get resources from extractor
        let resources = match extractor.get_resources() {
            Some(r) => r,
            None => return Ok(()), // No resources, can't process XObjects
        };

        // Resolve indirect reference to resources if needed
        let resolved_resources = if let Some(ref_obj) = resources.as_reference() {
            match self.load_object(ref_obj) {
                Ok(obj) => obj,
                Err(_) => return Ok(()),
            }
        } else {
            resources.clone()
        };

        // Get XObject dictionary from resources
        let resources_dict = match resolved_resources.as_dict() {
            Some(dict) => dict,
            None => return Ok(()),
        };

        let xobject_obj = match resources_dict.get("XObject") {
            Some(obj) => obj,
            None => return Ok(()),
        };

        // Resolve indirect reference to XObject dictionary if needed
        let resolved_xobject_obj = if let Some(ref_obj) = xobject_obj.as_reference() {
            match self.load_object(ref_obj) {
                Ok(obj) => obj,
                Err(_) => return Ok(()),
            }
        } else {
            xobject_obj.clone()
        };

        let xobject_dict = match resolved_xobject_obj.as_dict() {
            Some(dict) => dict,
            None => return Ok(()),
        };

        // Get XObject reference
        let xobject_ref = match xobject_dict.get(name) {
            Some(obj) => match obj.as_reference() {
                Some(r) => r,
                None => return Ok(()),
            },
            None => return Ok(()),
        };

        // Cycle detection: skip if already processing this XObject
        if !extractor.can_process_xobject(xobject_ref) {
            return Ok(());
        }
        extractor.push_xobject(xobject_ref);

        // Load XObject
        let xobject = match self.load_object(xobject_ref) {
            Ok(obj) => obj,
            Err(e) => {
                extractor.pop_xobject();
                return Err(e);
            },
        };
        let xobject_dict = match xobject.as_dict() {
            Some(dict) => dict,
            None => {
                extractor.pop_xobject();
                return Err(Error::ParseError {
                    offset: 0,
                    reason: "XObject is not a dictionary".to_string(),
                });
            },
        };

        // Check type - only process Form XObjects, skip Images
        match xobject_dict.get("Subtype") {
            Some(subtype_obj) => {
                if let Some(subtype_name) = subtype_obj.as_name() {
                    if subtype_name != "Form" {
                        extractor.pop_xobject();
                        return Ok(()); // Not a Form XObject, skip
                    }
                } else {
                    extractor.pop_xobject();
                    return Ok(());
                }
            },
            None => {
                extractor.pop_xobject();
                return Ok(());
            },
        }

        // Get and decode the stream
        let stream_data = match self.decode_stream_with_encryption(&xobject, xobject_ref) {
            Ok(data) => data,
            Err(e) => {
                extractor.pop_xobject();
                return Err(e);
            },
        };

        // Parse operators from the stream
        let operators = match parse_content_stream(&stream_data) {
            Ok(ops) => ops,
            Err(e) => {
                extractor.pop_xobject();
                return Err(e);
            },
        };

        // Get transformation matrix (default to identity)
        let matrix = if let Some(matrix_obj) = xobject_dict.get("Matrix") {
            if let Some(array) = matrix_obj.as_array() {
                if array.len() >= 6 {
                    let mut matrix = Matrix::identity();
                    let mut values = [0.0f32; 6];
                    let mut valid = true;

                    for (i, val) in array.iter().take(6).enumerate() {
                        let num = if let Some(f) = val.as_real() {
                            f as f32
                        } else if let Some(i_val) = val.as_integer() {
                            i_val as f32
                        } else {
                            valid = false;
                            break;
                        };
                        values[i] = num;
                    }

                    if valid {
                        matrix.a = values[0];
                        matrix.b = values[1];
                        matrix.c = values[2];
                        matrix.d = values[3];
                        matrix.e = values[4];
                        matrix.f = values[5];
                        matrix
                    } else {
                        Matrix::identity()
                    }
                } else {
                    Matrix::identity()
                }
            } else {
                Matrix::identity()
            }
        } else {
            Matrix::identity()
        };

        // Save graphics state
        state_stack.save();

        // Finalize any pending path before processing XObject to isolate state
        if extractor.has_current_path() {
            extractor.end_path();
        }

        // Apply XObject transformation to CTM
        let state = state_stack.current_mut();
        state.ctm = state.ctm.multiply(&matrix);
        extractor.set_ctm(state.ctm);

        // Process operators from the XObject
        for op in operators {
            match op {
                // Graphics state operators
                Operator::SaveState => {
                    state_stack.save();
                },
                Operator::RestoreState => {
                    state_stack.restore();
                    extractor.update_from_state(state_stack.current());
                },
                Operator::Cm { a, b, c, d, e, f } => {
                    let state = state_stack.current_mut();
                    let new_matrix = Matrix { a, b, c, d, e, f };
                    state.ctm = state.ctm.multiply(&new_matrix);
                    extractor.set_ctm(state.ctm);
                },

                // Color and line style operators (same as in extract_paths)
                Operator::SetStrokeRgb { r, g, b } => {
                    extractor.set_stroke_color(Color::new(r, g, b));
                },
                Operator::SetStrokeGray { gray } => {
                    extractor.set_stroke_color(Color::new(gray, gray, gray));
                },
                Operator::SetStrokeCmyk { c, m, y, k } => {
                    let r = (1.0 - c) * (1.0 - k);
                    let g = (1.0 - m) * (1.0 - k);
                    let b = (1.0 - y) * (1.0 - k);
                    extractor.set_stroke_color(Color::new(r, g, b));
                },
                Operator::SetFillRgb { r, g, b } => {
                    extractor.set_fill_color(Color::new(r, g, b));
                },
                Operator::SetFillGray { gray } => {
                    extractor.set_fill_color(Color::new(gray, gray, gray));
                },
                Operator::SetFillCmyk { c, m, y, k } => {
                    let r = (1.0 - c) * (1.0 - k);
                    let g = (1.0 - m) * (1.0 - k);
                    let b = (1.0 - y) * (1.0 - k);
                    extractor.set_fill_color(Color::new(r, g, b));
                },
                Operator::SetLineWidth { width } => {
                    extractor.set_line_width(width);
                },
                Operator::SetLineCap { cap_style } => {
                    let cap = match cap_style {
                        1 => LineCap::Round,
                        2 => LineCap::Square,
                        _ => LineCap::Butt,
                    };
                    extractor.set_line_cap(cap);
                },
                Operator::SetLineJoin { join_style } => {
                    let join = match join_style {
                        1 => LineJoin::Round,
                        2 => LineJoin::Bevel,
                        _ => LineJoin::Miter,
                    };
                    extractor.set_line_join(join);
                },

                // Path construction operators
                Operator::MoveTo { x, y } => extractor.move_to(x, y),
                Operator::LineTo { x, y } => extractor.line_to(x, y),
                Operator::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                } => {
                    extractor.curve_to(x1, y1, x2, y2, x3, y3);
                },
                Operator::CurveToV { x2, y2, x3, y3 } => {
                    extractor.curve_to_v(x2, y2, x3, y3);
                },
                Operator::CurveToY { x1, y1, x3, y3 } => {
                    extractor.curve_to_y(x1, y1, x3, y3);
                },
                Operator::Rectangle {
                    x,
                    y,
                    width,
                    height,
                } => {
                    extractor.rectangle(x, y, width, height);
                },
                Operator::ClosePath => extractor.close_path(),

                // Path painting operators
                Operator::Stroke => extractor.stroke(),
                Operator::Fill => extractor.fill(FillRule::NonZero),
                Operator::FillEvenOdd => extractor.fill(FillRule::EvenOdd),
                Operator::CloseFillStroke => extractor.close_fill_and_stroke(FillRule::NonZero),
                Operator::EndPath => extractor.end_path(),

                // Clipping operators
                Operator::ClipNonZero => extractor.clip_non_zero(),
                Operator::ClipEvenOdd => extractor.clip_even_odd(),

                // Nested XObjects (recurse)
                Operator::Do { name: nested_name } => {
                    if let Err(e) =
                        self.process_form_xobject_paths(&nested_name, extractor, state_stack)
                    {
                        log::warn!("Failed to process nested XObject '{}': {}", nested_name, e);
                    }
                },

                // Skip other operators
                _ => {},
            }
        }

        // Finalize any pending path to prevent state leakage
        if extractor.has_current_path() {
            extractor.end_path();
        }

        // Restore graphics state
        state_stack.restore();
        extractor.update_from_state(state_stack.current());

        // Pop from XObject processing stack
        extractor.pop_xobject();

        Ok(())
    }

    /// Extract paths from a specific rectangular region of a page.
    ///
    /// Only paths whose bounding box intersects the specified region are returned.
    ///
    /// # Arguments
    ///
    /// * `page_index` - Zero-based page index
    /// * `region` - The rectangular region to extract from
    ///
    /// # Returns
    ///
    /// A vector of `PathContent` objects within the specified region.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # use pdf_oxide::geometry::Rect;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("example.pdf")?;
    ///
    /// // Extract paths from a specific region (e.g., header area)
    /// let header_region = Rect::new(0.0, 700.0, 612.0, 92.0);
    /// let paths = doc.extract_paths_in_rect(0, header_region)?;
    ///
    /// println!("Found {} paths in header region", paths.len());
    /// # Ok(())
    /// # }
    /// ```
    pub fn extract_paths_in_rect(
        &mut self,
        page_index: usize,
        region: crate::geometry::Rect,
    ) -> Result<Vec<crate::elements::PathContent>> {
        let paths = self.extract_paths(page_index)?;

        // Filter paths by region intersection
        Ok(paths
            .into_iter()
            .filter(|path| path.bbox.intersects(&region))
            .collect())
    }

    /// Get information about a page, including its dimensions.
    ///
    /// This is useful for rendering and layout calculations.
    #[cfg(feature = "rendering")]
    pub fn get_page_info(&mut self, page_index: usize) -> Result<PageInfo> {
        let page = self.get_page(page_index)?;
        let page_dict = page.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "Page is not a dictionary".to_string(),
        })?;

        // Helper to extract f32 from Integer or Real
        fn obj_to_f32(obj: &Object) -> Option<f32> {
            match obj {
                Object::Integer(i) => Some(*i as f32),
                Object::Real(r) => Some(*r as f32),
                _ => None,
            }
        }

        // Get MediaBox (required, may be inherited)
        let media_box = page_dict
            .get("MediaBox")
            .and_then(|o| o.as_array())
            .map(|arr| {
                let x0 = arr.first().and_then(obj_to_f32).unwrap_or(0.0);
                let y0 = arr.get(1).and_then(obj_to_f32).unwrap_or(0.0);
                let x1 = arr.get(2).and_then(obj_to_f32).unwrap_or(612.0);
                let y1 = arr.get(3).and_then(obj_to_f32).unwrap_or(792.0);
                crate::geometry::Rect::from_points(x0, y0, x1, y1)
            })
            .unwrap_or(crate::geometry::Rect::from_points(
                0.0, 0.0, 612.0, 792.0, // Letter size default
            ));

        // Get CropBox (optional, falls back to MediaBox)
        let crop_box = page_dict
            .get("CropBox")
            .and_then(|o| o.as_array())
            .map(|arr| {
                let x0 = arr.first().and_then(obj_to_f32).unwrap_or(0.0);
                let y0 = arr.get(1).and_then(obj_to_f32).unwrap_or(0.0);
                let x1 = arr.get(2).and_then(obj_to_f32).unwrap_or(612.0);
                let y1 = arr.get(3).and_then(obj_to_f32).unwrap_or(792.0);
                crate::geometry::Rect::from_points(x0, y0, x1, y1)
            });

        // Get rotation (optional, default 0)
        let rotation = page_dict
            .get("Rotate")
            .and_then(|o| match o {
                Object::Integer(i) => Some(*i as i32),
                _ => None,
            })
            .unwrap_or(0);

        Ok(PageInfo {
            media_box,
            crop_box,
            rotation,
        })
    }

    /// Get the resources dictionary for a page.
    ///
    /// Resources contain fonts, images, patterns, and other objects
    /// used when rendering the page.
    #[cfg(feature = "rendering")]
    pub fn get_page_resources(&mut self, page_index: usize) -> Result<Object> {
        let page = self.get_page(page_index)?;
        let page_dict = page.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "Page is not a dictionary".to_string(),
        })?;

        // Get Resources (required, may be inherited)
        let resources = page_dict
            .get("Resources")
            .cloned()
            .unwrap_or(Object::Dictionary(std::collections::HashMap::new()));

        // If it's a reference, resolve it
        if let Some(ref_val) = resources.as_reference() {
            self.load_object(ref_val)
        } else {
            Ok(resources)
        }
    }

    /// Resolve an object reference.
    ///
    /// This is useful when working with indirect object references
    /// in content streams or resource dictionaries.
    #[cfg(feature = "rendering")]
    pub fn resolve_object(&mut self, obj: &Object) -> Result<Object> {
        if let Some(ref_val) = obj.as_reference() {
            self.load_object(ref_val)
        } else {
            Ok(obj.clone())
        }
    }

    /// Load fonts from a Resources dictionary into the extractor.
    fn load_fonts(
        &mut self,
        resources: &Object,
        extractor: &mut crate::extractors::TextExtractor,
    ) -> Result<()> {
        use crate::fonts::FontInfo;

        // Resources can be a reference or a dictionary
        let resources_obj = if let Some(res_ref) = resources.as_reference() {
            self.load_object(res_ref)?
        } else {
            resources.clone()
        };

        let resources_dict = resources_obj.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "Resources is not a dictionary".to_string(),
        })?;

        // Get Font dictionary if present
        if let Some(font_obj) = resources_dict.get("Font") {
            // Font can be a reference or direct dictionary - need to dereference
            let font_dict_obj = if let Some(font_ref) = font_obj.as_reference() {
                self.load_object(font_ref)?
            } else {
                font_obj.clone()
            };

            if let Some(font_dict) = font_dict_obj.as_dict() {
                for (name, font_obj) in font_dict {
                    // Font can be a reference or direct object
                    let font = if let Some(font_ref) = font_obj.as_reference() {
                        self.load_object(font_ref)?
                    } else {
                        font_obj.clone()
                    };

                    // Parse font info
                    match FontInfo::from_dict(&font, self) {
                        Ok(font_info) => {
                            extractor.add_font(name.clone(), font_info);
                        },
                        Err(e) => {
                            // Log font parsing failures for diagnostics
                            log::error!(
                                "Failed to load font '{}': {}. Text using this font will use fallback encoding.",
                                name,
                                e
                            );
                            continue;
                        },
                    }
                }
            }
        }

        Ok(())
    }

    /// Convert a page to Markdown format.
    ///
    /// Extracts text from the specified page and converts it to Markdown with
    /// optional heading detection and image references.
    ///
    /// # Arguments
    ///
    /// * `page_index` - Zero-based page index
    /// * `options` - Conversion options controlling the output
    ///
    /// # Returns
    ///
    /// A string containing the Markdown representation of the page.
    ///
    /// # Errors
    ///
    /// Returns an error if the page cannot be accessed or conversion fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pdf_oxide::PdfDocument;
    /// use pdf_oxide::converters::ConversionOptions;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("paper.pdf")?;
    ///
    /// let options = ConversionOptions {
    ///     detect_headings: true,
    ///     ..Default::default()
    /// };
    /// let markdown = doc.to_markdown(0, &options)?;
    /// println!("{}", markdown);
    /// # Ok(())
    /// # }
    /// ```
    #[allow(clippy::wrong_self_convention)] // Needs mutable access for caching
    pub fn to_markdown(
        &mut self,
        page_index: usize,
        options: &crate::converters::ConversionOptions,
    ) -> Result<String> {
        use crate::structure::traversal::extract_reading_order;

        // Step 1: Extract raw spans (unchanged - this is the foundation)
        let spans = self.extract_spans(page_index)?;

        // Step 2: Create pipeline config from options (using adapter from Phase 2)
        let pipeline_config = TextPipelineConfig::from_conversion_options(options);

        // Step 3: Handle structure tree context for reading order
        // Try to extract MCID order for StructureTreeFirst mode
        if let Ok(Some(struct_tree)) = self.structure_tree() {
            match extract_reading_order(&struct_tree, page_index as u32) {
                Ok(mcid_order) if !mcid_order.is_empty() => {
                    // Update context with extracted MCIDs
                    log::debug!(
                        "Extracted {} MCIDs from structure tree for page {}",
                        mcid_order.len(),
                        page_index
                    );
                },
                _ => {
                    // No MCIDs found - that's OK, fallback will happen in strategy
                    log::debug!(
                        "No MCIDs found for page {}, reading order strategy will use geometric fallback",
                        page_index
                    );
                },
            }
        } else {
            log::debug!(
                "No structure tree found, reading order strategy will use geometric fallback"
            );
        }

        // Step 4: Create pipeline with config
        let pipeline = TextPipeline::with_config(pipeline_config.clone());

        // Step 5: Build reading order context
        let context = ReadingOrderContext::new().with_page(page_index as u32);

        // Step 6: Process through pipeline (applies reading order strategy)
        let ordered_spans = pipeline.process(spans, context)?;

        // Step 7: Use pipeline converter
        let converter = MarkdownOutputConverter::new();
        let mut markdown = converter.convert(&ordered_spans, &pipeline_config)?;

        // Step 8: Extract and include images if enabled
        if options.include_images {
            let images = self.extract_images(page_index).unwrap_or_default();
            if !images.is_empty() {
                let image_markdown = self.generate_image_markdown(&images, options, page_index)?;
                markdown.push_str(&image_markdown);
            }
        }

        Ok(markdown)
    }

    /// Generate Markdown for extracted images.
    fn generate_image_markdown(
        &self,
        images: &[crate::extractors::PdfImage],
        options: &crate::converters::ConversionOptions,
        page_index: usize,
    ) -> Result<String> {
        use std::path::Path;

        let mut markdown = String::new();
        markdown.push_str("\n\n---\n\n");

        for (i, image) in images.iter().enumerate() {
            let alt = format!("Image {} from page {}", i + 1, page_index + 1);

            if options.embed_images {
                // Embed as base64 data URI (works in Obsidian, Typora, VS Code, Jupyter, etc.)
                match image.to_base64_data_uri() {
                    Ok(data_uri) => {
                        markdown.push_str(&format!("![{}]({})\n\n", alt, data_uri));
                    },
                    Err(e) => {
                        log::warn!("Failed to encode image {}: {}", i, e);
                    },
                }
            } else if let Some(ref output_dir) = options.image_output_dir {
                // Save to file and reference by path
                let filename = format!("page{}_{}.png", page_index + 1, i + 1);
                let filepath = Path::new(output_dir).join(&filename);

                // Create directory if needed
                if let Some(parent) = filepath.parent() {
                    std::fs::create_dir_all(parent).ok();
                }

                match image.save_as_png(&filepath) {
                    Ok(()) => {
                        let relative_path = format!("{}/{}", output_dir, filename);
                        markdown.push_str(&format!("![{}]({})\n\n", alt, relative_path));
                    },
                    Err(e) => {
                        log::warn!("Failed to save image {}: {}", i, e);
                    },
                }
            }
            // If embed_images=false and no output_dir, skip image
        }

        Ok(markdown)
    }

    /// Convert a page to Markdown with automatic OCR fallback for scanned pages.
    ///
    /// This method automatically detects scanned pages and applies OCR when needed,
    /// falling back to native text extraction for regular PDFs.
    ///
    /// **Note**: Requires the `ocr` feature to be enabled and OCR models to be provided.
    ///
    /// # Arguments
    ///
    /// * `page_index` - Zero-based page index
    /// * `options` - Conversion options controlling the output
    /// * `ocr_engine` - Optional OCR engine (required for scanned pages)
    /// * `ocr_options` - OCR extraction options
    ///
    /// # Returns
    ///
    /// A string containing the Markdown representation of the page.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use pdf_oxide::{PdfDocument, ocr::{OcrEngine, OcrConfig, OcrExtractOptions}};
    /// use pdf_oxide::converters::ConversionOptions;
    ///
    /// let mut doc = PdfDocument::open("scanned.pdf")?;
    /// let engine = OcrEngine::new("det.onnx", "rec.onnx", "dict.txt", OcrConfig::default())?;
    ///
    /// let markdown = doc.to_markdown_with_ocr(
    ///     0,
    ///     &ConversionOptions::default(),
    ///     Some(&engine),
    ///     &OcrExtractOptions::default()
    /// )?;
    /// ```
    #[cfg(feature = "ocr")]
    pub fn to_markdown_with_ocr(
        &mut self,
        page_index: usize,
        options: &crate::converters::ConversionOptions,
        ocr_engine: Option<&crate::ocr::OcrEngine>,
        ocr_options: &crate::ocr::OcrExtractOptions,
    ) -> Result<String> {
        #[allow(deprecated)]
        use crate::converters::{MarkdownConverter, ReadingOrderMode};
        use crate::structure::traversal::extract_reading_order;

        // Extract spans with OCR fallback
        let spans = self.extract_spans_with_ocr(page_index, ocr_engine, ocr_options)?;
        #[allow(deprecated)]
        let converter = MarkdownConverter::new();

        // Check if we need to extract structure tree for StructureTreeFirst mode
        let mut options = options.clone();
        if matches!(options.reading_order_mode, ReadingOrderMode::StructureTreeFirst { .. }) {
            // Try to parse structure tree and extract MCID reading order
            if let Ok(Some(struct_tree)) = self.structure_tree() {
                match extract_reading_order(&struct_tree, page_index as u32) {
                    Ok(mcid_order) if !mcid_order.is_empty() => {
                        // Update reading order mode with extracted MCIDs
                        options.reading_order_mode =
                            ReadingOrderMode::StructureTreeFirst { mcid_order };
                        log::debug!(
                            "Extracted {} MCIDs from structure tree for page {}",
                            match &options.reading_order_mode {
                                ReadingOrderMode::StructureTreeFirst { mcid_order } =>
                                    mcid_order.len(),
                                _ => 0,
                            },
                            page_index
                        );
                    },
                    _ => {
                        // No MCIDs found or error - fallback to ColumnAware
                        log::debug!(
                            "No MCIDs found for page {}, using ColumnAware fallback",
                            page_index
                        );
                        options.reading_order_mode =
                            ReadingOrderMode::StructureTreeFirst { mcid_order: vec![] };
                    },
                }
            } else {
                // No structure tree - fallback to ColumnAware
                log::debug!("No structure tree found, using ColumnAware fallback");
                options.reading_order_mode =
                    ReadingOrderMode::StructureTreeFirst { mcid_order: vec![] };
            }
        }

        // Use the new PDF spec compliant span-based converter
        converter.convert_page_from_spans(&spans, &options)
    }

    /// Convert a page to HTML format.
    ///
    /// Extracts text from the specified page and converts it to HTML.
    /// Supports both semantic HTML and layout-preserved modes based on options.
    ///
    /// # Arguments
    ///
    /// * `page_index` - Zero-based page index
    /// * `options` - Conversion options controlling the output
    ///
    /// # Returns
    ///
    /// A string containing the HTML representation of the page.
    ///
    /// # Errors
    ///
    /// Returns an error if the page cannot be accessed or conversion fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pdf_oxide::PdfDocument;
    /// use pdf_oxide::converters::ConversionOptions;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("paper.pdf")?;
    ///
    /// // Semantic HTML
    /// let options = ConversionOptions::default();
    /// let html = doc.to_html(0, &options)?;
    ///
    /// // Layout-preserved HTML
    /// let layout_options = ConversionOptions {
    ///     preserve_layout: true,
    ///     ..Default::default()
    /// };
    /// let layout_html = doc.to_html(0, &layout_options)?;
    /// # Ok(())
    /// # }
    /// ```
    #[allow(clippy::wrong_self_convention)] // Needs mutable access for caching
    pub fn to_html(
        &mut self,
        page_index: usize,
        options: &crate::converters::ConversionOptions,
    ) -> Result<String> {
        // Step 1: Extract raw spans (unchanged - this is the foundation)
        let spans = self.extract_spans(page_index)?;

        // Step 2: Create pipeline config from options (using adapter from Phase 2)
        let pipeline_config = TextPipelineConfig::from_conversion_options(options);

        // Step 3: Create pipeline with config
        let pipeline = TextPipeline::with_config(pipeline_config.clone());

        // Step 4: Build reading order context
        let context = ReadingOrderContext::new().with_page(page_index as u32);

        // Step 5: Process through pipeline (applies reading order strategy)
        let ordered_spans = pipeline.process(spans, context)?;

        // Step 6: Use pipeline converter
        let converter = HtmlOutputConverter::new();
        let mut html = converter.convert(&ordered_spans, &pipeline_config)?;

        // Step 7: Extract and embed images if enabled
        if options.include_images {
            let images = self.extract_images(page_index).unwrap_or_default();
            if !images.is_empty() {
                let image_html = self.generate_image_html(&images, options, page_index)?;
                // Insert images before closing </body> or at end
                if let Some(pos) = html.rfind("</body>") {
                    html.insert_str(pos, &image_html);
                } else {
                    html.push_str(&image_html);
                }
            }
        }

        Ok(html)
    }

    /// Generate HTML for extracted images.
    fn generate_image_html(
        &self,
        images: &[crate::extractors::PdfImage],
        options: &crate::converters::ConversionOptions,
        page_index: usize,
    ) -> Result<String> {
        use std::path::Path;

        let mut html = String::new();
        html.push_str("\n<div class=\"page-images\">\n");

        for (i, image) in images.iter().enumerate() {
            let alt = format!("Image {} from page {}", i + 1, page_index + 1);

            if options.embed_images {
                // Embed as base64 data URI
                match image.to_base64_data_uri() {
                    Ok(data_uri) => {
                        html.push_str(&format!(
                            "  <img src=\"{}\" alt=\"{}\" style=\"max-width: 100%;\">\n",
                            data_uri, alt
                        ));
                    },
                    Err(e) => {
                        // Log error but continue with other images
                        log::warn!("Failed to encode image {}: {}", i, e);
                    },
                }
            } else if let Some(ref output_dir) = options.image_output_dir {
                // Save to file and reference by path
                let filename = format!("page{}_{}.png", page_index + 1, i + 1);
                let filepath = Path::new(output_dir).join(&filename);

                // Create directory if needed
                if let Some(parent) = filepath.parent() {
                    std::fs::create_dir_all(parent).ok();
                }

                match image.save_as_png(&filepath) {
                    Ok(()) => {
                        // Use relative path in HTML
                        let relative_path = format!("{}/{}", output_dir, filename);
                        html.push_str(&format!(
                            "  <img src=\"{}\" alt=\"{}\" style=\"max-width: 100%;\">\n",
                            relative_path, alt
                        ));
                    },
                    Err(e) => {
                        log::warn!("Failed to save image {}: {}", i, e);
                    },
                }
            }
            // If embed_images=false and no output_dir, skip image
        }

        html.push_str("</div>\n");
        Ok(html)
    }

    /// Convert a page to plain text.
    ///
    /// Extracts text from the specified page with minimal formatting.
    /// This is equivalent to calling `extract_text()`.
    ///
    /// # Arguments
    ///
    /// * `page_index` - Zero-based page index
    /// * `options` - Conversion options (currently unused for plain text, reserved for future use)
    ///
    /// # Returns
    ///
    /// A string containing the plain text content of the page.
    ///
    /// # Errors
    ///
    /// Returns an error if the page cannot be accessed or text extraction fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pdf_oxide::PdfDocument;
    /// use pdf_oxide::converters::ConversionOptions;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("paper.pdf")?;
    /// let options = ConversionOptions::default();
    /// let text = doc.to_plain_text(0, &options)?;
    /// println!("{}", text);
    /// # Ok(())
    /// # }
    /// ```
    #[allow(clippy::wrong_self_convention)] // Needs mutable access for caching
    pub fn to_plain_text(
        &mut self,
        page_index: usize,
        options: &crate::converters::ConversionOptions,
    ) -> Result<String> {
        // Step 1: Extract raw spans (unchanged - this is the foundation)
        let spans = self.extract_spans(page_index)?;

        // Step 2: Create pipeline config from options (using adapter from Phase 2)
        let pipeline_config = TextPipelineConfig::from_conversion_options(options);

        // Step 3: Create pipeline with config
        let pipeline = TextPipeline::with_config(pipeline_config.clone());

        // Step 4: Build reading order context
        let context = ReadingOrderContext::new().with_page(page_index as u32);

        // Step 5: Process through pipeline (applies reading order strategy)
        let ordered_spans = pipeline.process(spans, context)?;

        // Step 6: Use pipeline converter
        let converter = PlainTextConverter::new();
        converter.convert(&ordered_spans, &pipeline_config)
    }

    /// Convert all pages to Markdown format.
    ///
    /// Extracts and converts all pages in the document to Markdown,
    /// separating pages with "---" horizontal rules.
    ///
    /// # Arguments
    ///
    /// * `options` - Conversion options controlling the output
    ///
    /// # Returns
    ///
    /// A string containing the Markdown representation of all pages.
    ///
    /// # Errors
    ///
    /// Returns an error if any page cannot be accessed or conversion fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pdf_oxide::PdfDocument;
    /// use pdf_oxide::converters::ConversionOptions;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("paper.pdf")?;
    /// let options = ConversionOptions::default();
    /// let markdown = doc.to_markdown_all(&options)?;
    /// # Ok(())
    /// # }
    /// ```
    #[allow(clippy::wrong_self_convention)] // Needs mutable access for caching
    pub fn to_markdown_all(
        &mut self,
        options: &crate::converters::ConversionOptions,
    ) -> Result<String> {
        let page_count = self.page_count()?;
        let mut result = String::new();

        for i in 0..page_count {
            if i > 0 {
                result.push_str("\n---\n\n");
            }
            let page_markdown = self.to_markdown(i, options)?;
            result.push_str(&page_markdown);
        }

        Ok(result)
    }

    /// Convert all pages to plain text format.
    ///
    /// Extracts all pages in the document as plain text,
    /// separating pages with "---" horizontal rules.
    ///
    /// # Arguments
    ///
    /// * `options` - Conversion options (currently unused for plain text, reserved for future use)
    ///
    /// # Returns
    ///
    /// A string containing the plain text of all pages.
    ///
    /// # Errors
    ///
    /// Returns an error if any page cannot be accessed or extraction fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pdf_oxide::PdfDocument;
    /// use pdf_oxide::converters::ConversionOptions;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("paper.pdf")?;
    /// let options = ConversionOptions::default();
    /// let text = doc.to_plain_text_all(&options)?;
    /// # Ok(())
    /// # }
    /// ```
    #[allow(clippy::wrong_self_convention)] // Needs mutable access for caching
    pub fn to_plain_text_all(
        &mut self,
        options: &crate::converters::ConversionOptions,
    ) -> Result<String> {
        let page_count = self.page_count()?;
        let mut result = String::new();

        for i in 0..page_count {
            if i > 0 {
                result.push_str("\n\n---\n\n");
            }
            let page_text = self.to_plain_text(i, options)?;
            result.push_str(&page_text);
        }

        Ok(result)
    }

    /// Check for circular references in the object graph.
    ///
    /// This is a diagnostic method that performs a depth-first search
    /// through the object graph to detect cycles.
    ///
    /// # Returns
    ///
    /// A vector of tuples representing edges that create cycles.
    /// Each tuple is (from_object, to_object) where to_object is
    /// already in the path when encountered again.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # let mut doc = PdfDocument::open("sample.pdf")?;
    /// let cycles = doc.check_for_circular_references();
    /// if !cycles.is_empty() {
    ///     println!("Found {} circular references", cycles.len());
    /// }
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn check_for_circular_references(&mut self) -> Vec<(ObjectRef, ObjectRef)> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut path = Vec::new();

        // Check all objects in the xref table
        let obj_nums: Vec<u32> = self.xref.entries.keys().copied().collect();
        for obj_num in obj_nums {
            let obj_ref = ObjectRef::new(obj_num, 0);
            if !visited.contains(&obj_ref) {
                self.dfs_check_cycles(obj_ref, &mut visited, &mut path, &mut cycles);
            }
        }

        cycles
    }

    /// Depth-first search helper for cycle detection.
    fn dfs_check_cycles(
        &mut self,
        obj_ref: ObjectRef,
        visited: &mut HashSet<ObjectRef>,
        path: &mut Vec<ObjectRef>,
        cycles: &mut Vec<(ObjectRef, ObjectRef)>,
    ) {
        if path.contains(&obj_ref) {
            // Found cycle
            if let Some(&prev) = path.last() {
                cycles.push((prev, obj_ref));
            }
            return;
        }

        if visited.contains(&obj_ref) {
            return;
        }

        visited.insert(obj_ref);
        path.push(obj_ref);

        // Get object and scan for references
        if let Ok(obj) = self.load_object(obj_ref) {
            for ref_found in Self::find_references(&obj) {
                self.dfs_check_cycles(ref_found, visited, path, cycles);
            }
        }

        path.pop();
    }

    /// Find all object references within an object.
    fn find_references(obj: &Object) -> Vec<ObjectRef> {
        let mut refs = Vec::new();

        match obj {
            Object::Reference(r) => refs.push(*r),
            Object::Array(arr) => {
                for item in arr {
                    refs.extend(Self::find_references(item));
                }
            },
            Object::Dictionary(dict) => {
                for value in dict.values() {
                    refs.extend(Self::find_references(value));
                }
            },
            Object::Stream { dict, .. } => {
                for value in dict.values() {
                    refs.extend(Self::find_references(value));
                }
            },
            _ => {},
        }

        refs
    }

    /// Convert all pages to HTML format.
    ///
    /// Extracts and converts all pages in the document to HTML,
    /// wrapping each page in a div with class "page".
    ///
    /// # Arguments
    ///
    /// * `options` - Conversion options controlling the output
    ///
    /// # Returns
    ///
    /// A string containing the HTML representation of all pages.
    ///
    /// # Errors
    ///
    /// Returns an error if any page cannot be accessed or conversion fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pdf_oxide::PdfDocument;
    /// use pdf_oxide::converters::ConversionOptions;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut doc = PdfDocument::open("paper.pdf")?;
    /// let options = ConversionOptions::default();
    /// let html = doc.to_html_all(&options)?;
    /// # Ok(())
    /// # }
    /// ```
    #[allow(clippy::wrong_self_convention)] // Needs mutable access for caching
    pub fn to_html_all(
        &mut self,
        options: &crate::converters::ConversionOptions,
    ) -> Result<String> {
        let page_count = self.page_count()?;
        let mut result = String::new();

        for i in 0..page_count {
            result.push_str(&format!("<div class=\"page\" data-page=\"{}\">\n", i + 1));
            let page_html = self.to_html(i, options)?;
            result.push_str(&page_html);
            result.push_str("</div>\n");
        }

        Ok(result)
    }

    /// Extract images from a page.
    ///
    /// Extracts all images from the specified page by processing the content stream.
    /// This includes:
    /// - Images referenced via `Do` operators (XObject calls)
    /// - Images in nested Form XObjects (with recursion)
    /// - Inline images (BI...ID...EI sequences)
    ///
    /// This method processes PDF content streams instead of only iterating the XObject
    /// dictionary. This ensures that images referenced via the `Do` operator in the content
    /// stream are properly extracted, including those in nested Form XObjects. ColorSpace
    /// indirect references are also resolved.
    ///
    /// Returns a vector of PdfImage objects representing the extracted images.
    ///
    /// # Arguments
    ///
    /// * `page_index` - Zero-based page index
    ///
    /// # Returns
    ///
    /// A vector of PdfImage objects, one for each image found on the page.
    ///
    /// # Errors
    ///
    /// Returns an error if the page cannot be accessed or if image extraction fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use pdf_oxide::document::PdfDocument;
    /// # let mut doc = PdfDocument::open("sample.pdf")?;
    /// let images = doc.extract_images(0)?;
    /// println!("Found {} images on page 1", images.len());
    /// for (i, image) in images.iter().enumerate() {
    ///     image.save_as_png(&format!("image_{}.png", i))?;
    /// }
    /// # Ok::<(), pdf_oxide::error::Error>(())
    /// ```
    pub fn extract_images(
        &mut self,
        page_index: usize,
    ) -> Result<Vec<crate::extractors::PdfImage>> {
        use crate::content::parse_content_stream;
        use crate::content::Operator;

        // Get page object and resources
        let page = self.get_page(page_index)?;
        let page_dict = page.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "Page is not a dictionary".to_string(),
        })?;

        // Get content stream
        let content_data = self.get_page_content_data(page_index)?;

        // Resolve resources
        let resources = match page_dict.get("Resources") {
            Some(res) => {
                if let Some(ref_obj) = res.as_reference() {
                    Some(self.load_object(ref_obj)?)
                } else {
                    Some(res.clone())
                }
            },
            None => None,
        };

        // Parse content stream and extract images
        let operators = match parse_content_stream(&content_data) {
            Ok(ops) => ops,
            Err(_) => {
                // If content stream parsing fails, return empty
                return Ok(Vec::new());
            },
        };

        let mut images = Vec::new();
        let mut ctm_stack = vec![crate::content::Matrix::identity()];

        // Parse content stream operators to extract images from Do operators
        // Instead of only checking the XObject dictionary, we parse the actual page content
        // stream to find Do operators that reference images. This is how real PDFs work -
        // images are embedded as XObjects and referenced via "Do" operators in the content stream.
        for op in operators {
            match op {
                // Graphics state operators
                Operator::SaveState => {
                    if let Some(current_ctm) = ctm_stack.last() {
                        ctm_stack.push(*current_ctm);
                    }
                },
                Operator::RestoreState => {
                    if ctm_stack.len() > 1 {
                        ctm_stack.pop();
                    }
                },
                Operator::Cm { a, b, c, d, e, f } => {
                    if let Some(current_ctm) = ctm_stack.last_mut() {
                        let matrix = crate::content::Matrix { a, b, c, d, e, f };
                        *current_ctm = current_ctm.multiply(&matrix);
                    }
                },

                // XObject reference operator - Extract images referenced via Do
                // The "Do" operator tells the renderer: "Draw the named XObject now"
                // We extract all images referenced this way
                Operator::Do { name } => {
                    if let Some(ref res) = resources {
                        let current_ctm = ctm_stack
                            .last()
                            .copied()
                            .unwrap_or_else(crate::content::Matrix::identity);
                        if let Ok(mut xobj_images) =
                            self.extract_images_from_xobject_do(&name, res, current_ctm)
                        {
                            images.append(&mut xobj_images);
                        }
                    }
                },

                // Inline image operator
                Operator::InlineImage { dict, data } => {
                    let current_ctm = ctm_stack
                        .last()
                        .copied()
                        .unwrap_or_else(crate::content::Matrix::identity);
                    if let Ok(image) = self.extract_image_from_inline(&dict, &data, current_ctm) {
                        images.push(image);
                    }
                },

                _ => {}, // Ignore other operators
            }
        }

        Ok(images)
    }

    /// Extract images referenced by a Do operator in the content stream.
    ///
    /// This method handles image extraction from XObjects. It processes both Image and Form
    /// XObjects, with recursion for nested Forms.
    ///
    /// PDF files embed images as XObjects rather than inline images. These XObjects are
    /// referenced in the page's content stream via the `Do` operator. For example: `/ImgName Do`
    /// tells the renderer to draw the image named "ImgName".
    ///
    /// This method:
    /// - Locates XObject references in the Resources dictionary
    /// - Resolves both direct and indirect references (e.g., `7 0 R`)
    /// - Extracts Image XObjects directly
    /// - Recursively processes Form XObjects
    /// - Applies CTM transformations for proper positioning
    /// - Resolves ColorSpace indirect references
    fn extract_images_from_xobject_do(
        &mut self,
        name: &str,
        resources: &Object,
        ctm: crate::content::Matrix,
    ) -> Result<Vec<crate::extractors::PdfImage>> {
        use crate::extractors::extract_image_from_xobject;

        let mut images = Vec::new();

        // Get XObject dictionary
        let resources_dict = resources.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "Resources is not a dictionary".to_string(),
        })?;

        let xobject_obj = match resources_dict.get("XObject") {
            Some(obj) => obj,
            None => return Ok(images), // No XObjects, return empty
        };

        // Resolve indirect reference if needed
        let resolved_xobject_obj = if let Some(ref_obj) = xobject_obj.as_reference() {
            self.load_object(ref_obj)?
        } else {
            xobject_obj.clone()
        };

        let xobject_dict = resolved_xobject_obj
            .as_dict()
            .ok_or_else(|| Error::ParseError {
                offset: 0,
                reason: "XObject dictionary is not a dictionary".to_string(),
            })?;

        // Get the specific XObject by name
        let xobject_ref_obj = match xobject_dict.get(name) {
            Some(obj) => obj,
            None => return Ok(images), // Named XObject not found
        };

        // Load XObject (can be indirect reference or direct object)
        let xobject_ref_opt = xobject_ref_obj.as_reference();
        let xobject = if let Some(ref_obj) = xobject_ref_opt {
            self.load_object(ref_obj)?
        } else {
            xobject_ref_obj.clone()
        };
        let xobject_dict = xobject.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "XObject is not a dictionary".to_string(),
        })?;

        // Check Subtype
        let subtype = xobject_dict
            .get("Subtype")
            .and_then(|s| s.as_name())
            .unwrap_or("");

        match subtype {
            "Image" => {
                // For Stream objects, resolve any indirect references in the dictionary
                let mut resolved_xobject = xobject.clone();

                if let Object::Stream { dict, data } = &xobject {
                    let mut new_dict = dict.clone();

                    // Resolve ColorSpace if it's an indirect reference
                    // Many PDFs from tools like Google Slides reference ColorSpace via indirect
                    // references (e.g., "7 0 R"). The extraction function needs the resolved object.
                    // Without this, we get: "Invalid color space object: Reference(...)"
                    if let Some(Object::Reference(cs_ref)) = dict.get("ColorSpace") {
                        if let Ok(resolved_cs) = self.load_object(*cs_ref) {
                            new_dict.insert("ColorSpace".to_string(), resolved_cs);
                        }
                    }

                    resolved_xobject = Object::Stream {
                        dict: new_dict,
                        data: data.clone(),
                    };
                }

                // Extract as Image XObject
                if let Ok(mut image) =
                    extract_image_from_xobject(Some(self), &resolved_xobject, xobject_ref_opt)
                {
                    if let Some(rect) = image.bbox() {
                        let new_bbox = self.transform_bbox_with_ctm(rect, ctm);
                        image.set_bbox(new_bbox);
                    } else {
                        let width = image.width() as f32;
                        let height = image.height() as f32;
                        let bbox = crate::geometry::Rect {
                            x: ctm.e,
                            y: ctm.f,
                            width: ctm.a * width,
                            height: ctm.d * height,
                        };
                        image.set_bbox(bbox);
                    }
                    images.push(image);
                }
            },
            "Form" => {
                // Recursively extract from Form XObject
                // Only process if we have a valid reference
                if let Some(ref_obj) = xobject_ref_opt {
                    if let Ok(mut form_images) = self.extract_images_from_form_xobject(
                        ref_obj,
                        &xobject,
                        resources,
                        ctm,
                        &mut Vec::new(),
                    ) {
                        images.append(&mut form_images);
                    }
                }
            },
            _ => {}, // Skip other types (PS, etc.)
        }

        Ok(images)
    }

    /// Recursively extract images from a Form XObject.
    fn extract_images_from_form_xobject(
        &mut self,
        xobject_ref: ObjectRef,
        xobject: &Object,
        parent_resources: &Object,
        parent_ctm: crate::content::Matrix,
        xobject_stack: &mut Vec<ObjectRef>,
    ) -> Result<Vec<crate::extractors::PdfImage>> {
        use crate::content::parse_content_stream;
        use crate::content::Operator;

        let mut images = Vec::new();

        // Cycle detection
        if xobject_stack.contains(&xobject_ref) || xobject_stack.len() >= 100 {
            return Ok(images);
        }
        xobject_stack.push(xobject_ref);

        let xobject_dict = xobject.as_dict().ok_or_else(|| Error::ParseError {
            offset: 0,
            reason: "Form XObject is not a dictionary".to_string(),
        })?;

        // Get Form resources (with fallback to parent)
        let form_resources = if let Some(form_res) = xobject_dict.get("Resources") {
            if let Some(ref_obj) = form_res.as_reference() {
                self.load_object(ref_obj)?
            } else {
                form_res.clone()
            }
        } else {
            parent_resources.clone()
        };

        // Get Form transformation matrix (default to identity)
        let matrix = if let Some(matrix_obj) = xobject_dict.get("Matrix") {
            self.parse_matrix_from_object(matrix_obj)
                .unwrap_or_else(crate::content::Matrix::identity)
        } else {
            crate::content::Matrix::identity()
        };

        // Combine transformations
        let new_ctm = parent_ctm.multiply(&matrix);

        // Decode form stream
        let stream_data = self.decode_stream_with_encryption(xobject, xobject_ref)?;

        // Parse operators from form stream
        let operators = parse_content_stream(&stream_data)?;

        // Process operators (similar to extract_images_from_content)
        let mut ctm_stack = vec![new_ctm];

        for op in operators {
            match op {
                Operator::SaveState => {
                    if let Some(current_ctm) = ctm_stack.last() {
                        ctm_stack.push(*current_ctm);
                    }
                },
                Operator::RestoreState => {
                    if ctm_stack.len() > 1 {
                        ctm_stack.pop();
                    }
                },
                Operator::Cm { a, b, c, d, e, f } => {
                    if let Some(current_ctm) = ctm_stack.last_mut() {
                        let matrix = crate::content::Matrix { a, b, c, d, e, f };
                        *current_ctm = current_ctm.multiply(&matrix);
                    }
                },

                Operator::Do { name } => {
                    let current_ctm = ctm_stack
                        .last()
                        .copied()
                        .unwrap_or_else(crate::content::Matrix::identity);
                    if let Ok(mut xobj_images) =
                        self.extract_images_from_xobject_do(&name, &form_resources, current_ctm)
                    {
                        images.append(&mut xobj_images);
                    }
                },

                Operator::InlineImage { dict, data } => {
                    let current_ctm = ctm_stack
                        .last()
                        .copied()
                        .unwrap_or_else(crate::content::Matrix::identity);
                    if let Ok(image) = self.extract_image_from_inline(&dict, &data, current_ctm) {
                        images.push(image);
                    }
                },

                _ => {}, // Ignore other operators
            }
        }

        xobject_stack.pop();
        Ok(images)
    }

    /// Extract an inline image from the content stream.
    fn extract_image_from_inline(
        &mut self,
        dict: &std::collections::HashMap<String, Object>,
        data: &[u8],
        ctm: crate::content::Matrix,
    ) -> Result<crate::extractors::PdfImage> {
        use crate::extractors::expand_inline_image_dict;

        // Expand abbreviated dictionary
        let expanded_dict = expand_inline_image_dict(dict.clone());

        // Build a temporary stream object from the dictionary and data
        let stream_obj = Object::Stream {
            dict: expanded_dict,
            data: bytes::Bytes::copy_from_slice(data),
        };

        // Use existing extraction logic
        let mut image =
            crate::extractors::extract_image_from_xobject(Some(self), &stream_obj, None)?;

        // Apply CTM to create bbox
        let width = image.width() as f32;
        let height = image.height() as f32;
        let bbox = crate::geometry::Rect {
            x: ctm.e,
            y: ctm.f,
            width: ctm.a * width,
            height: ctm.d * height,
        };
        image.set_bbox(bbox);

        Ok(image)
    }

    /// Transform a bounding box using CTM.
    fn transform_bbox_with_ctm(
        &self,
        rect: &crate::geometry::Rect,
        ctm: crate::content::Matrix,
    ) -> crate::geometry::Rect {
        // Transform the corners of the bbox using the CTM
        // Bottom-left corner
        let x1 = ctm.a * rect.x + ctm.c * rect.y + ctm.e;
        let y1 = ctm.b * rect.x + ctm.d * rect.y + ctm.f;

        // Top-right corner
        let x2 = ctm.a * (rect.x + rect.width) + ctm.c * (rect.y + rect.height) + ctm.e;
        let y2 = ctm.b * (rect.x + rect.width) + ctm.d * (rect.y + rect.height) + ctm.f;

        // Create new bbox from transformed corners
        let x = x1.min(x2);
        let y = y1.min(y2);
        let width = (x1 - x2).abs();
        let height = (y1 - y2).abs();

        crate::geometry::Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Parse a Matrix object from PDF.
    fn parse_matrix_from_object(&self, obj: &Object) -> Option<crate::content::Matrix> {
        if let Some(array) = obj.as_array() {
            if array.len() >= 6 {
                let mut values = [0.0f32; 6];
                for (i, val) in array.iter().take(6).enumerate() {
                    let num = if let Some(f) = val.as_real() {
                        f as f32
                    } else if let Some(i_val) = val.as_integer() {
                        i_val as f32
                    } else {
                        return None;
                    };
                    values[i] = num;
                }

                return Some(crate::content::Matrix {
                    a: values[0],
                    b: values[1],
                    c: values[2],
                    d: values[3],
                    e: values[4],
                    f: values[5],
                });
            }
        }
        None
    }

    /// Extract images from a page and save them to files.
    ///
    /// Each image is saved as a separate file in `output_dir` with the given
    /// `prefix` and an incrementing index starting from `start_index`.
    pub fn extract_images_to_files(
        &mut self,
        page_index: usize,
        output_dir: impl AsRef<Path>,
        prefix: Option<&str>,
        start_index: Option<usize>,
    ) -> Result<Vec<ExtractedImageRef>> {
        use std::fs;

        // Extract images from page
        let images = self.extract_images(page_index)?;

        // Create output directory if it doesn't exist
        let output_dir = output_dir.as_ref();
        if !output_dir.exists() {
            fs::create_dir_all(output_dir).map_err(Error::Io)?;
        }

        let prefix = prefix.unwrap_or("img");
        let mut index = start_index.unwrap_or(1);
        let mut result = Vec::new();

        for image in images {
            // Determine format and extension
            let (format, extension) = match image.data() {
                crate::extractors::ImageData::Jpeg(_) => (ImageFormat::Jpeg, "jpg"),
                _ => (ImageFormat::Png, "png"),
            };

            // Generate filename: img_001.png, img_002.jpg, etc.
            let filename = format!("{}_{:03}.{}", prefix, index, extension);
            let filepath = output_dir.join(&filename);

            // Save image
            match format {
                ImageFormat::Jpeg => image.save_as_jpeg(&filepath)?,
                ImageFormat::Png => image.save_as_png(&filepath)?,
            }

            // Add to result
            result.push(ExtractedImageRef {
                filename,
                format,
                width: image.width(),
                height: image.height(),
            });

            index += 1;
        }

        Ok(result)
    }
}

/// Reference to an extracted image file.
///
/// Contains metadata about an image that has been extracted and saved to a file.
/// Used for HTML export to embed images with correct dimensions and format.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedImageRef {
    /// Filename of the saved image (e.g., "img_001.png")
    pub filename: String,
    /// Image format
    pub format: ImageFormat,
    /// Image width in pixels
    pub width: u32,
    /// Image height in pixels
    pub height: u32,
}

/// Image format for extracted images.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// PNG format (lossless)
    Png,
    /// JPEG format (lossy, preserves DCT-encoded images)
    Jpeg,
}

/// Parse PDF header (%PDF-x.y) from a reader.
///
/// # Arguments
///
/// * `reader` - A readable and seekable source (e.g., File, Cursor)
/// * `lenient` - If false, fail if header not at byte 0; if true, search first 8192 bytes
///
/// # Returns
///
/// Returns `Ok((major, minor, offset))` with the PDF version and byte offset where header was found.
/// In strict mode, offset will be 0 if successful. In lenient mode, offset may be > 0 for PDFs
/// with leading binary data (compliant with ISO 32000-1:2008, page 41).
///
/// # Examples
///
/// ```rust
/// use std::io::Cursor;
/// # use pdf_oxide::document::parse_header;
///
/// let data = b"%PDF-1.7\n";
/// let mut cursor = Cursor::new(data);
/// let (major, minor, offset) = parse_header(&mut cursor, false).unwrap();
/// assert_eq!((major, minor, offset), (1, 7, 0));
/// ```
pub fn parse_header<R: Read + Seek>(reader: &mut R, lenient: bool) -> Result<(u8, u8, u64)> {
    // Try to get current position
    let start_pos = reader.stream_position().unwrap_or(0);

    // Read first 8 bytes for fast path (header at byte 0)
    let mut header = [0u8; 8];
    let strict_read_ok = match reader.read_exact(&mut header) {
        Ok(_) => {
            // Check if header is at position 0
            if &header[0..5] == b"%PDF-" {
                return parse_version_from_header(&header, false)
                    .map(|(major, minor)| (major, minor, 0));
            }
            true
        },
        Err(e) => {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                // File too short for PDF header
                if !lenient {
                    return Err(Error::InvalidHeader(
                        "File too short for PDF header (expected at least 8 bytes)".to_string(),
                    ));
                }
                false
            } else {
                return Err(Error::InvalidHeader(format!("Failed to read file: {}", e)));
            }
        },
    };

    // If strict mode and first 8 bytes read, fail immediately
    if !lenient && strict_read_ok {
        return Err(Error::InvalidHeader(format!(
            "Expected '%PDF-' at byte 0, found '{}'",
            String::from_utf8_lossy(&header[0..5])
        )));
    }

    // Lenient mode: search first 8192 bytes
    reader.seek(SeekFrom::Start(start_pos))?;

    // Read up to 8192 bytes
    let mut buffer = vec![0u8; 8192];
    let bytes_read = match reader.read(&mut buffer) {
        Ok(0) => return Err(Error::InvalidHeader("File is empty (0 bytes read)".to_string())),
        Ok(n) => n,
        Err(e) => {
            return Err(Error::InvalidHeader(format!(
                "I/O error while searching for PDF header: {}",
                e
            )))
        },
    };

    buffer.truncate(bytes_read);

    // Search for "%PDF-" marker
    match find_substring(&buffer, b"%PDF-") {
        Some(offset) => {
            // Verify we have enough bytes for the version
            if offset + 8 > buffer.len() {
                return Err(Error::InvalidHeader(
                    "PDF header found but insufficient bytes for version".to_string(),
                ));
            }

            let header_bytes = &buffer[offset..offset + 8];
            let mut header_arr = [0u8; 8];
            header_arr.copy_from_slice(header_bytes);

            let (major, minor) = parse_version_from_header(&header_arr, true)?;

            // Standardize reader position to just after the header
            // (consistent with strict mode behavior at line 4378)
            let header_start = start_pos + offset as u64;
            let after_header = header_start + 8;
            reader.seek(SeekFrom::Start(after_header))?;

            Ok((major, minor, header_start))
        },
        None => Err(Error::InvalidHeader(
            "No PDF header found in first 8192 bytes of file".to_string(),
        )),
    }
}

/// Parse version information from a header buffer.
/// Assumes buffer starts with "%PDF-" and has at least 8 bytes.
///
/// When `lenient` is true, malformed version strings (e.g., `%PDF-1.\n`, `%PDF-a.4`)
/// default to version (1, 4) instead of returning an error.
fn parse_version_from_header(header: &[u8; 8], lenient: bool) -> Result<(u8, u8)> {
    // Check magic bytes "%PDF-"
    if &header[0..5] != b"%PDF-" {
        return Err(Error::InvalidHeader(format!(
            "Expected '%PDF-', found '{}'",
            String::from_utf8_lossy(&header[0..5])
        )));
    }

    // Parse version (e.g., "1.7")
    // Format: %PDF-M.m where M is major version (1 digit), m is minor version (1 digit)
    if header[6] != b'.' {
        if lenient {
            log::warn!(
                "Malformed PDF version format (expected '.', found '{}'), defaulting to 1.4",
                header[6] as char
            );
            return Ok((1, 4));
        }
        return Err(Error::InvalidHeader(format!(
            "Invalid version format: expected '.', found '{}'",
            header[6] as char
        )));
    }

    let major = header[5];
    let minor = header[7];

    // Validate digits
    if !major.is_ascii_digit() || !minor.is_ascii_digit() {
        if lenient {
            log::warn!(
                "Malformed PDF version '{}.{}' (non-digit characters), defaulting to 1.4",
                major as char,
                minor as char
            );
            return Ok((1, 4));
        }
        return Err(Error::InvalidHeader(format!(
            "Invalid version: {}.{} (not digits)",
            major as char, minor as char
        )));
    }

    let major = major - b'0';
    let minor = minor - b'0';

    // Validate version range (PDF 1.0 - 2.0)
    if major > 2 || (major == 0 && minor == 0) {
        if lenient {
            log::warn!("Unsupported PDF version {}.{}, defaulting to 1.4", major, minor);
            return Ok((1, 4));
        }
        return Err(Error::UnsupportedVersion(format!("{}.{}", major, minor)));
    }

    Ok((major, minor))
}

/// Parse the trailer dictionary from a reader.
///
/// The trailer comes immediately after the xref table and before "startxref".
/// It starts with the keyword "trailer" followed by a dictionary.
///
/// # Example Format
///
/// ```text
/// trailer
/// << /Size 6 /Root 1 0 R /Info 5 0 R >>
/// startxref
/// 1234
/// %%EOF
/// ```
///
/// # Arguments
///
/// * `reader` - A readable source positioned after the xref table
///
/// # Returns
///
/// Returns the trailer dictionary as an `Object`.
///
/// # Errors
///
/// Returns an error if:
/// - The "trailer" keyword is not found
/// - The dictionary following "trailer" cannot be parsed
/// - The reader encounters an I/O error
pub fn parse_trailer<R: Read>(reader: &mut R) -> Result<Object> {
    // The reader should already be positioned after the xref table
    // We need to read until we find "trailer", then parse the dictionary

    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;

    // Find "trailer" keyword
    let content = String::from_utf8_lossy(&buffer);
    let trailer_pos = content.find("trailer").ok_or_else(|| {
        Error::InvalidPdf("Trailer keyword not found after xref table".to_string())
    })?;

    // Skip past "trailer" keyword (7 bytes)
    let dict_start = trailer_pos + 7;
    if dict_start >= buffer.len() {
        return Err(Error::UnexpectedEof);
    }

    // Parse the dictionary that follows
    let (_, trailer_dict) = parse_object(&buffer[dict_start..]).map_err(|e| Error::ParseError {
        offset: dict_start,
        reason: format!("Failed to parse trailer dictionary: {:?}", e),
    })?;

    // Verify it's a dictionary
    if trailer_dict.as_dict().is_none() {
        return Err(Error::InvalidPdf("Trailer is not a dictionary".to_string()));
    }

    Ok(trailer_dict)
}

/// Find the first occurrence of a substring in a byte slice.
///
/// Returns the index of the first occurrence, or None if not found.
fn find_substring(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_valid_header_1_7() {
        let mut cursor = Cursor::new(b"%PDF-1.7\n");
        let (major, minor, offset) = parse_header(&mut cursor, false).unwrap();
        assert_eq!((major, minor, offset), (1, 7, 0));
    }

    #[test]
    fn test_parse_valid_header_1_4() {
        let mut cursor = Cursor::new(b"%PDF-1.4");
        let (major, minor, offset) = parse_header(&mut cursor, false).unwrap();
        assert_eq!((major, minor, offset), (1, 4, 0));
    }

    #[test]
    fn test_parse_valid_header_1_0() {
        let mut cursor = Cursor::new(b"%PDF-1.0");
        let (major, minor, offset) = parse_header(&mut cursor, false).unwrap();
        assert_eq!((major, minor, offset), (1, 0, 0));
    }

    #[test]
    fn test_parse_valid_header_2_0() {
        let mut cursor = Cursor::new(b"%PDF-2.0");
        let (major, minor, offset) = parse_header(&mut cursor, false).unwrap();
        assert_eq!((major, minor, offset), (2, 0, 0));
    }

    #[test]
    fn test_parse_invalid_header_wrong_magic_strict() {
        let mut cursor = Cursor::new(b"NotAPDF\n");
        let result = parse_header(&mut cursor, false);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InvalidHeader(_)));
    }

    #[test]
    fn test_parse_invalid_header_unsupported_version() {
        let mut cursor = Cursor::new(b"%PDF-3.0");
        let result = parse_header(&mut cursor, false);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::UnsupportedVersion(_)));
    }

    #[test]
    fn test_parse_invalid_header_version_0_0() {
        let mut cursor = Cursor::new(b"%PDF-0.0");
        let result = parse_header(&mut cursor, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_header_no_dot() {
        let mut cursor = Cursor::new(b"%PDF-17\n");
        let result = parse_header(&mut cursor, false);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InvalidHeader(_)));
    }

    #[test]
    fn test_parse_invalid_header_too_short() {
        let mut cursor = Cursor::new(b"%PDF");
        let result = parse_header(&mut cursor, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_header_non_digit_version() {
        let mut cursor = Cursor::new(b"%PDF-X.Y");
        let result = parse_header(&mut cursor, false);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InvalidHeader(_)));
    }

    // ========================================================================
    // Header parsing tests with various prefixes
    #[test]
    fn test_parse_header_with_bom_prefix() {
        // UTF-8 BOM prefix before header
        let data = b"\xEF\xBB\xBF%PDF-1.7\n";
        let mut cursor = Cursor::new(data);
        let (major, minor, offset) = parse_header(&mut cursor, true).unwrap();
        assert_eq!((major, minor, offset), (1, 7, 3));
    }

    #[test]
    fn test_parse_header_with_binary_prefix() {
        // Binary data prefix before header
        let mut data = vec![0x1b, 0x96, 0x5f];
        data.extend_from_slice(b"%PDF-1.4\n");
        let mut cursor = Cursor::new(data);
        let (major, minor, offset) = parse_header(&mut cursor, true).unwrap();
        assert_eq!((major, minor, offset), (1, 4, 3));
    }

    #[test]
    fn test_parse_header_at_boundary() {
        // Header starting at byte 1016 (within 1024-byte window, with 8 bytes for full header)
        let mut data = vec![0u8; 1016];
        data.extend_from_slice(b"%PDF-1.5");
        let mut cursor = Cursor::new(data);
        let (major, minor, offset) = parse_header(&mut cursor, true).unwrap();
        assert_eq!((major, minor, offset), (1, 5, 1016));
    }

    #[test]
    fn test_parse_header_not_found_lenient() {
        // No header in first 1024 bytes, lenient mode should fail
        let data = vec![0u8; 1024];
        let mut cursor = Cursor::new(data);
        let result = parse_header(&mut cursor, true);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_header_strict_rejects_offset() {
        // With binary prefix but strict mode should fail
        let mut data = vec![0x1b, 0x96, 0x5f];
        data.extend_from_slice(b"%PDF-1.4\n");
        let mut cursor = Cursor::new(data);
        let result = parse_header(&mut cursor, false);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InvalidHeader(_)));
    }

    // ========================================================================
    // Trailer Parsing Tests
    // ========================================================================

    #[test]
    fn test_parse_trailer_basic() {
        let data = b"trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n";
        let mut cursor = Cursor::new(data);
        let trailer = parse_trailer(&mut cursor).unwrap();

        let dict = trailer.as_dict().unwrap();
        assert_eq!(dict.get("Size").unwrap().as_integer(), Some(6));
        assert!(dict.get("Root").unwrap().as_reference().is_some());
    }

    #[test]
    fn test_parse_trailer_missing_keyword() {
        let data = b"<< /Size 6 >>\nstartxref\n";
        let mut cursor = Cursor::new(data);
        let result = parse_trailer(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_trailer_not_dictionary() {
        let data = b"trailer\n[ 1 2 3 ]\nstartxref\n";
        let mut cursor = Cursor::new(data);
        let result = parse_trailer(&mut cursor);
        assert!(result.is_err());
    }

    // ========================================================================
    // PdfDocument Error Tests
    // ========================================================================

    #[test]
    fn test_document_open_nonexistent_file() {
        let result = PdfDocument::open("/nonexistent/path/to/file.pdf");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Io(_)));
    }

    #[test]
    fn test_circular_reference_detection() {
        // This test ensures that the cycle detection mechanism works
        // We can't easily create a circular PDF in a unit test, but we can
        // verify that the error types exist and are properly defined
        use crate::object::ObjectRef;

        let obj_ref = ObjectRef::new(1, 0);
        let err = Error::CircularReference(obj_ref);
        let msg = format!("{}", err);
        assert!(msg.contains("Circular reference"));
        assert!(msg.contains("object 1 0 R"));
    }

    #[test]
    fn test_recursion_limit_error() {
        let err = Error::RecursionLimitExceeded(100);
        let msg = format!("{}", err);
        assert!(msg.contains("Recursion depth limit exceeded"));
        assert!(msg.contains("100"));
    }
}
