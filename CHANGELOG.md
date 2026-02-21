# Changelog

All notable changes to PDFOxide are documented here.

## [0.3.8] - 2026-02-20
> Performance: Text-Only Parser — Graphics-Heavy Pages 10-30x Faster

### Performance

- **Text-only content stream parser** (#110) — New `parse_content_stream_text_only()` fast path skips graphics operators outside BT/ET blocks using byte-level scanning instead of full nom parsing. Only text-affecting operators are returned.

- **Byte-level graphics scanner** (#112) — Replaced nom-based operand loop with raw index arithmetic in `scan_graphics_region()`. Processes digits, dots, and whitespace at near-memcpy speed, skipping path coordinates without constructing any Objects.

- **Skip color operators in scanner** (#114) — Added 12 color operators (`rg`, `RG`, `g`, `G`, `k`, `K`, `cs`, `CS`, `sc`, `SC`, `scn`, `SCN`) to the byte-level skip list. Pure color state changes never affect text content or positioning.

- **Defer q/cm/Q emission until text confirmed** (#116) — Graphics state save/restore (`q`/`Q`) and CTM transforms (`cm`) outside BT/ET are deferred rather than immediately parsed. If a `q...Q` block contains no text-triggering operator (BT, BI, Do), all its state ops are silently discarded. When text IS found, the deferred region is re-parsed to preserve CTM. Eliminates ~75% of remaining backtrack overhead on graphics-heavy pages.

- **Arc-wrap FontInfo cache** (#111) — Font cache entries wrapped in `Arc` to avoid cloning full FontInfo structs. Removed eager CMap validation that blocked font loading on partially-valid CMap streams.

- **O(n) page map construction** — Rewrote `build_page_map` as single-pass traversal with parse budgets, replacing recursive descent that could degenerate on deeply nested page trees.

- **Structure tree optimization** — Arc-cached structure tree with batch traversal; skip ParentTree parsing for non-tagged content.

- **XObject name→ref cache** — Cache XObject dictionary lookups to eliminate O(n²) dictionary cloning on pages with many XObject references.

### Verified — 3,829-PDF Corpus

- **0 new errors** — All 3,829 PDFs extract successfully (7 pre-existing failures on intentionally broken test fixtures)
- **1,712 unit tests passing** — Zero failures, zero warnings
- **Clippy clean** — `cargo clippy -- -D warnings` passes

### Issues Resolved

| Closes | Description |
|--------|-------------|
| #110 | Add text-only content stream parser fast path |
| #111 | Arc-wrap FontInfo cache, remove eager CMap validation |
| #112 | Replace nom-based operand loop with byte-level scanner |
| #114 | Skip color operators in graphics scanner |
| #116 | Defer q/cm/Q emission until text is confirmed |

## [0.3.7] - 2026-02-19
> Text Extraction Quality: 95.7% to 99.6% Clean Rate

### Verified — 3,829-PDF Corpus (v0.3.6 → v0.3.7)

| Metric | v0.3.6 | v0.3.7 | Change |
|--------|--------|--------|--------|
| **Clean rate** | 95.7% | **99.6%** | 3,812 of 3,829 PDFs |
| **Dirty PDFs** | 165 | **17** | **-90%** |

Systematic benchmark testing across 3,829 real-world PDFs identified and fixed 13 text extraction issues.

### Added — Parser & Decoders

- **BrotliDecode stream filter** (PDF 2.0, ISO 32000-2:2020) — New `BrotliDecoder` for PDFs using Brotli-compressed streams (#95)
- **Xref trailer selection** — Improve selection of the correct trailer when multiple trailers exist, fixing files where the wrong trailer was selected
- **Headerless PDF recovery** — Search for first object marker when `%PDF-` header is missing
- **Multi-line object headers** — Handle `1 0\nobj` format used by Google-generated PDFs
- **Cross-reference stream reconstruction** — Rebuild xref from object markers for damaged PDFs

### Added — Font Encoding

- **CFF font encoding parser** (`src/fonts/cff_encoding.rs`) — Parse CFF/OpenType font programs to extract character encoding when no ToUnicode CMap is present (#87, #99)
- **Type1 font encoding parser** (`src/fonts/type1_encoding.rs`) — Parse embedded Type 1 font programs for `/Encoding` arrays with `dup CODE /GLYPHNAME put` patterns (#89)
- **80K+ CID-to-Unicode mappings** — Expanded Adobe-CNS1 (+18K), Adobe-GB1 (+30K), Adobe-Japan1 (+15K), Adobe-Korea1 (+17K) character collections (#98)
- **Shift-JIS/RKSJ decoding** — Added `encoding_rs` dependency for Japanese Shift-JIS encoded CMap streams (#100)
- **TeX math glyph names** — Map MSAM, MSBM, and Computer Modern glyph names to Unicode equivalents
- **Identity-H cmap propagation** — Propagate TrueType cmap tables from CIDFont descendants to Type0 parent fonts (#91)
- **Cross-font cmap sharing** — Share TrueType cmap tables across Identity-H fonts that lack embedded encoding data (#91)

### Fixed — Text Extraction Pipeline

- **Tf buffer flush** — Flush pending text buffer on font switch (`Tf` operator) to prevent text loss when multiple fonts are used in the same text block (#88)
- **Adaptive space threshold** — Replace fixed 0.25em threshold with bbox-based adaptive spacing, eliminating spurious spaces in tightly-set text (#97)
- **Span deduplication** — Deduplicate overlapping text spans rendered multiple times at the same position (used for bold/shadow effects in some PDFs) (#102)
- **Character deduplication** — Remove duplicate characters within 2pt horizontal distance on the same line (#102)
- **BT operator check removal** — Remove incorrect content stream validation that silently skipped valid text blocks, causing empty output (#101)
- **ByteMode decoding** — Properly handle 1-byte, 2-byte (Identity-H/UCS2), and variable-width (Shift-JIS) character code decoding (#100, #103)
- **Annotation text extraction** — Extract text from Widget (form field), FreeText, and appearance stream annotations (#92)
- **Metadata string filtering** — Filter leaked WhitePoint, BlackPoint, and CalRGB metadata from extracted text output
- **ToUnicode control character fallback** — Fall back to font encoding when ToUnicode maps to control characters

### Fixed — Font Handling

- **TrueType cmap format 4** — Fix off-by-one in segment endCode comparison for format 4 lookup tables (#98)
- **CMap byte-width detection** — Detect CMap input code width from `begincodespacerange` for proper multi-byte decoding
- **CMap bfrange array targets** — Handle `bfrange` entries with array targets (mapping ranges to non-contiguous Unicode sequences)
- **Symbolic font encoding** — Correct encoding resolution order for symbolic fonts without explicit `/Encoding`

### Added — Tooling

- **Benchmark suite** — `bench_extract_all` example for corpus-wide extraction benchmarking
- **Comparison scripts** — `bench_compare.py` (pdf_oxide vs PyMuPDF), `bench_pymupdf.py`, `export_text_comparison.py` for side-by-side quality analysis
- **Regression tests** — 11 new regression tests in `test_v037_regressions.rs` covering all major fixes

### Issues Resolved

| Closes | Description |
|--------|-------------|
| #87 | Custom encoding producing garbage text |
| #88 | Multi-font text loss on Tf switch |
| #89 | Type1 subset font encoding not parsed |
| #91 | Identity-H fonts missing cmap propagation |
| #92 | Annotation/form field text not extracted |
| #95 | BrotliDecode stream filter not supported |
| #97 | Spurious spaces from fixed threshold |
| #98 | CID/ToUnicode producing U+FFFD replacements |
| #99 | Font encoding offset errors |
| #100 | Raw bytes emitted instead of decoded text |
| #101 | Empty output from valid content streams |
| #102 | Overlapping duplicate text not deduplicated |
| #103 | Character fragmentation from byte-width errors |

Ref #90, #93, #94, #96, #104, #105

## [0.3.6] - 2026-02-16
> Performance: Two Critical O(n) Bottlenecks Eliminated

### Performance

- **Bulk page tree cache** — On first page access, the entire page tree is walked once and all pages are cached. Previously `get_page()` traversed from root for every uncached page — O(n) per page, O(n²) total for sequential access. Now O(1) per page after a single O(n) walk.
  - **isartor-6-1-12-t01-fail-a.pdf (10,000 pages): 55,667ms → 332ms (168× faster)**
  - Eliminates the last >5s PDF in the entire 3,830-file corpus

- **Scan-for-object offset cache** (#44) — When objects are missing from the xref table, `scan_for_object()` previously read the entire PDF file for each missing object. Tagged PDFs with hundreds of structure tree elements not in xref triggered hundreds of full file reads. Now the file is scanned once and all object offsets are cached in a HashMap.
  - **Artikeltext (10pp, 1.3MB): 9,931ms → 68ms (146× faster)**
  - **cs231n (154pp, 571 fonts): 17,872ms → 405ms (44× faster)**

- **Single-pass text extraction** — `extract_spans()` no longer runs two passes (classify document type, then extract). The classification pass was discarded entirely; adaptive font-aware thresholds now produce equal or better results in a single pass.

- **Content stream Vec pre-allocation** — `parse_content_stream()` pre-allocates operator Vec capacity based on stream size (`data.len() / 20`), reducing reallocations for large content streams.

### Verified — 3,830-PDF Corpus (v0.3.5 → v0.3.6)

| Metric | v0.3.5 | v0.3.6 | Change |
|--------|--------|--------|--------|
| **Pass rate** | 99.8% | 99.8% | 3,823 of 3,830 valid PDFs |
| **Slow (>5s)** | 2 | **0** | Eliminated |
| **Mean** | 23.3ms | **2.1ms** | **-91%** |
| **p50** | 0.6ms | 0.6ms | — |
| **p90** | 3.0ms | **2.6ms** | -13% |
| **p95** | 5.1ms | **4.7ms** | -8% |
| **p99** | 33.2ms | **18.0ms** | **-46%** |
| **Max** | 68,722ms | **625ms** | **-99%** |
| **Sum (all PDFs)** | 89.1s | **8.0s** | **-91%** |

The 7 non-passing files are intentionally broken test fixtures (missing PDF header, fuzz-corrupted catalogs, invalid xref streams).

Text output verified byte-identical on 11 PDFs (862KB of extracted text). 4 PDFs show improved extraction quality from adaptive spacing (more complete words recovered).

### 🏆 Community Contributors

🥇 **@SeanPedersen** — Continued thanks to Sean whose Issue #44 performance report directly drove the investigation that uncovered both O(n) bottlenecks. His real-world test PDFs (German academic papers, Stanford lecture slides) were instrumental in profiling and validating the fixes. 🙏📊

## [0.3.5] - 2026-02-15
> Performance, 3,830-PDF Stability & Error Recovery

### Performance

- **Font caching across pages** — Document-level font cache keyed by `ObjectRef` avoids re-parsing shared fonts on every page. For a 1000-page document sharing 20 fonts, this reduces font parsing from 40,000 operations to 20
- **Page object caching** — `get_page()` caches resolved page objects in a `HashMap<usize, Object>`, eliminating repeated page tree traversal for multi-page extraction
- **Structure tree caching** — Structure tree result cached after first access, avoiding redundant parsing on every `extract_text()` call (major impact on tagged PDFs like PDF32000_2008.pdf)
- **BT operator early-out** — `extract_spans()`, `extract_spans_with_config()`, and `extract_chars()` skip the full text extraction pipeline for image-only pages that contain no `BT` (Begin Text) operators
- **Larger I/O buffer for big files** — `BufReader` capacity increased from 8 KB to 256 KB for files >100 MB, reducing syscall overhead on 1.5 GB newspaper archives
- **Xref reconstruction threshold removed** — Eliminated the `xref.len() < 5` heuristic that triggered full-file reconstruction on valid portfolio PDFs with few objects (5-13s → <100ms)

### Verified — 3,830-PDF Corpus

- **100% pass rate** on 3,830 PDFs across three independent test suites: veraPDF (2,907), Mozilla pdf.js (897), SafeDocs (26)
- **Zero timeouts, zero panics** — every PDF completes within 120 seconds
- **p50 = 0.6ms, p90 = 3.0ms, p99 = 33ms** — 97.6% of PDFs complete in under 10ms
- Added `verify_corpus` example binary for reproducible batch verification with CSV output, timeout handling, and per-corpus breakdown

### Added - Encryption

- **Owner password authentication** (Algorithm 7 for R≤4, Algorithm 12 for R≥5)
  - R≤4: Derives RC4 key from owner password via MD5 hash chain, decrypts `/O` value to recover user password, then validates via user password authentication
  - R≥5: SHA-256 verification with SASLprep normalization and owner validation/key salts per PDF spec §7.6.3.4
  - Both algorithms now fully wired into `EncryptionHandler::authenticate()`
- **R≥5 user password verification with SASLprep** — Full AES-256 password verification using SHA-256 with validation and key salts per PDF spec §7.6.4.3.3
- **Public password authentication API** — `Pdf::authenticate(password)` and `PdfDocument::authenticate(password)` exposed for user-facing password entry

### Added - PDF/A Compliance Validation

- **XMP metadata validation** — Parses XMP metadata stream and checks for `pdfaid:part` and `pdfaid:conformance` identification entries (clause 6.7.11)
- **Color space validation** — Scans page content streams for device-dependent color operators (`rg`, `RG`, `k`, `K`, `g`, `G`) without output intent (clause 6.2)
- **AFRelationship validation** — For PDF/A-3 documents with embedded files, validates each file specification dictionary contains the required `AFRelationship` key (clause 6.8)

### Added - PDF/X Compliance Validation

- **XMP PDF/X identification** — Parses XMP metadata for `pdfxid:GTS_PDFXVersion`, validates against declared level (clause 6.7.2)
- **Page box relationship validation** — Validates TrimBox ⊆ BleedBox ⊆ MediaBox and ArtBox ⊆ MediaBox with 0.01pt tolerance (clause 6.1.1)
- **ExtGState transparency detection** — Checks `SMask` (not `/None`), `CA`/`ca` < 1.0, and `BM` not `Normal`/`Compatible` in extended graphics state dictionaries (clause 6.3)
- **Device-dependent color detection** — Flags DeviceRGB/CMYK/Gray color spaces used without output intent (clause 6.2.3)
- **ICC profile validation** — Validates ICCBased color space profile streams contain required `/N` entry (clause 6.2.3)

### Added - Rendering

- **Spec-correct clipping** (PDF §8.5.4) — Clip state scoped to `q`/`Q` save/restore via clip stack; new clips intersect with existing clip region; `W`/`W*` no longer consume the current path (deferred to next paint operator); clip mask applied to all painting operations including text and images
- **Glyph advance width calculation** — Text position advances per PDF spec §9.4.4: `tx = (w0/1000 × Tfs + Tc + Tw) × Th` with 600-unit default glyph width
- **Form XObject rendering** — Parses `/Matrix` transform, uses form's `/Resources` (or inherits from parent), and recursively executes form content stream operators

### Fixed - Error Recovery (28+ real-world PDFs)

- **Missing objects resolve to Null** — Per PDF spec §7.3.10, unresolvable indirect references now return `Null` instead of errors, fixing 16 files across veraPDF/pdf.js corpora
- **Lenient header version parsing** — Fixed fast-path bug where valid headers with unusual version strings were rejected
- **Non-standard encryption algorithm matching** — V=1,R=3 combinations now handled leniently instead of rejected
- **Non-dictionary Resources** — Pages with invalid `/Resources` entries (e.g., Null, Integer) treated as empty resources instead of erroring
- **Null nodes in page tree** — Null or non-dictionary child nodes in page tree gracefully skipped during traversal
- **Corrupt content streams** — Malformed content streams return empty content instead of propagating parse errors
- **Enhanced page tree scanning** — `/Resources`+`/Parent` heuristic and `/Kids` direct resolution added as fallback passes for damaged page trees

### Fixed - DoS Protection

- **Bogus /Count bounds checking** — Page count validated against PDF spec Annex C.2 limit (8,388,607) and total object count; unreasonable values fall back to tree scanning

### Fixed - Image Extraction
- **Content stream image extraction** — `extract_images()` now processes page content streams to find `Do` operator calls, extracting images referenced via XObjects that were previously missed
- **Nested Form XObject images** — Recursive extraction with cycle detection handles images inside Form XObjects
- **Inline images** — `BI`...`ID`...`EI` sequences parsed with abbreviation expansion per PDF spec
- **CTM transformations** — Image bounding boxes correctly transformed using full 4-corner affine transform (handles rotation, shear, and negative scaling)
- **ColorSpace indirect references** — Resolved indirect references (e.g., `7 0 R`) in image color space entries before extraction

### Fixed - Parser Robustness

- **Multi-line object headers** — Parser now handles `1 0\nobj` format used by Google-generated PDFs instead of requiring `1 0 obj` on a single line
- **Extended header search** — Header search window extended from 1024 to 8192 bytes to handle PDFs with large binary prefixes
- **Lenient version parsing** — Malformed version strings like `%PDF-1.a` or truncated headers no longer cause parse failures in lenient mode

### Fixed - Page Access Robustness

- **Missing Contents entry** — Pages without a `/Contents` key now return empty content data instead of erroring
- **Cyclic page tree detection** — Page tree traversal tracks visited nodes to prevent stack overflow on malformed circular references
- **Null stream references** — Null or invalid stream references handled gracefully instead of panicking
- **Wider page scanning fallback** — Page scanning fallback triggers on more error conditions, improving compatibility with damaged PDFs
- **Pages without /Type entry** — Page scanning now finds pages missing the `/Type /Page` entry by checking for `/MediaBox` or `/Contents` keys

### Fixed - Encryption Robustness

- **Short encryption key panic** — AES decryption with undersized keys now returns an error instead of panicking
- **Xref stream parsing hardened** — Malformed xref streams with invalid entry sizes or out-of-bounds data no longer cause panics
- **Indirect /Encrypt references** — `/Encrypt` dictionary values that are indirect references are now resolved before parsing

### Fixed - Content Stream Processing

- **Dictionary-as-Stream fallback** — When a stream object is a bare dictionary (no stream data), it is now treated as an empty stream instead of causing a decode error
- **Filter abbreviations** — Abbreviated filter names (`AHx`, `A85`, `LZW`, `Fl`, `RL`, `CCF`, `DCT`) and case-insensitive matching now supported
- **Operator limit** — Content stream parsing enforces a configurable operator limit (default 1,000,000) to prevent pathological slowdowns on malformed streams

### Fixed - Code Quality

- **Structure tree indirect object references** — `ObjectRef` variants in structure tree `/K` entries are now resolved at parse time instead of being silently skipped, ensuring complete structure tree traversal
- **Lexer `R` token disambiguation** — `tag(b"R")` no longer matches the `R` prefix of `RG`/`ri`/`re` operators; `1 0 RG` is now correctly parsed as a color operator instead of indirect reference `1 0 R` + orphan `G`
- **Stream whitespace trimming** — `trim_leading_stream_whitespace` now only strips CR/LF (0x0D/0x0A), no longer strips NUL bytes (0x00) or spaces from binary stream data (fixes grayscale image extraction and object stream parsing)

### Tests

- **8 previously ignored tests un-ignored and fixed**:
  - `test_extract_raw_grayscale_image_from_xobject` — Fixed stream trimming stripping binary pixel data
  - `test_parse_object_stream_with_whitespace` — Fixed stream trimming affecting object stream offsets
  - `test_parse_object_stream_graceful_failure` — Relaxed assertion for improved parser recovery
  - `test_markdown_reading_order_top_to_bottom` — Fixed test coordinates to use PDF convention (Y increases upward)
  - `test_html_layout_multiple_elements` — Fixed assertions for per-character positioning
  - `test_reading_order_graph_based_simple` — Fixed test coordinates to PDF convention
  - `test_reading_order_two_columns` — Fixed test coordinates to PDF convention
  - `test_parse_color_operators` — Fixed lexer R/RG token disambiguation

### Removed

- Deleted empty `PdfImage` stub (`src/images.rs`) and its module export — image extraction uses `ImageInfo` from `src/extractors/images.rs`
- Deleted commented-out `DocumentType::detect()` test block in `src/extractors/gap_statistics.rs`
- Removed stale TODO comments in `scripts/setup-hooks.sh`, `src/bin/analyze_pdf_features.rs`, `src/document.rs`

### 🏆 Community Contributors

🥇 **@SeanPedersen** — Huge thanks to Sean for reporting multiple issues (#41, #44, #45, #46) that drove the entire stability focus of this release. His real-world testing uncovered a parser bug with Google-generated PDFs, image extraction failures on content stream references, and performance problems — each report triggering deep investigation and significant fixes. The parser robustness, image extraction, and testing infrastructure improvements in v0.3.5 all trace back to Sean's thorough bug reports. 🙏🔍

## [0.3.4] - 2026-02-12
> Parsing Robustness, Character Extraction & XObject Paths

### ⚠️ Breaking Changes
- **`parse_header()` function signature** - Now includes offset tracking
  - **Before**: `parse_header(reader) -> Result<(u8, u8)>`
  - **After**: `parse_header(reader, lenient) -> Result<(u8, u8, u64)>`
  - **Migration**: Replace `let (major, minor) = parse_header(&mut reader)?;` with `let (major, minor, _offset) = parse_header(&mut reader, true)?;`
  - Note: This is a public API function; consider using `doc.version()` for typical use cases instead

### Fixed - PDF Parsing Robustness (Issue #41)
- **Header offset support** - PDFs with binary prefixes or BOM headers now open successfully
  - Parse header function now searches first 1024 bytes for `%PDF-` marker (PDF spec compliant)
  - Supports UTF-8 BOM, email headers, and other leading binary data
  - `parse_header()` returns byte offset where header was found
  - Lenient mode (default) handles real-world malformed PDFs; strict mode for compliance testing
  - Fixes parsing errors like "expected '%PDF-', found '1b965'"

### Added - Character-Level Text Extraction (Issue #39)
- **`extract_chars()` API** - Low-level character-level extraction for layout analysis
  - Returns `Vec<TextChar>` with per-character positioning, font, and styling data
  - Includes transformation matrix, rotation angle, advance width
  - Sorted in reading order (top-to-bottom, left-to-right)
  - Overlapping characters (rendered multiple times) deduplicated
  - 30-50% faster than span extraction for character-only use cases
  - Exposed in both Rust and Python APIs
  - **Python binding**: `doc.extract_chars(page_index)` returns list of `TextChar` objects

### Added - XObject Path Extraction (Issue #40)
- **Form XObject support in path extraction** - Now extracts vectors from embedded XObjects
  - `extract_paths()` recursively processes Form XObjects via `Do` operator
  - Image XObjects properly skipped (only Form XObjects extracted)
  - Coordinate transformations via `/Matrix` properly applied
  - Graphics state properly isolated (save/restore)
  - Duplicate XObject detection prevents infinite loops
  - Nested XObjects (XObject containing XObject) supported

### Changed
- **Dependencies**: Upgraded nom parser library from 7.1 to 8.0
  - Updated all parser combinators to use `.parse()` method
  - No user-facing API changes
  - All parser functionality maintained
  - Performance stable (no regressions detected)
- `parse_header()` signature updated: now returns `(major, minor, offset)` tuple
- All parse_header test cases updated to use new signature

## [0.3.1] - 2026-01-14
> Form Fields, Multimedia & Python 3.8-3.14

### Added - Form Field Coverage (95% across Read/Create/Modify)

#### Hierarchical Field Creation
- **Parent/Child Field Structures** - Create complex form hierarchies like `address.street`, `address.city`
  - `add_parent_field()` - Create container fields without widgets
  - `add_child_field()` - Add child fields to existing parents
  - `add_form_field_hierarchical()` - Auto-create parent hierarchy from dotted names
  - `ParentFieldConfig` for configuring container fields
  - Property inheritance between parent and child fields (FT, V, DV, Ff, DA, Q)

#### Field Property Modification
- **Edit All Field Properties** - Beyond just values
  - `set_form_field_readonly()` / `set_form_field_required()` - Flag manipulation
  - `set_form_field_rect()` - Reposition/resize fields
  - `set_form_field_tooltip()` - Set hover text (TU)
  - `set_form_field_max_length()` - Text field length limits
  - `set_form_field_alignment()` - Text alignment (left/center/right)
  - `set_form_field_default_value()` - Default values (DV)
  - `BorderStyle` and `AppearanceCharacteristics` support
- **Critical Bug Fix** - Modified existing fields now persist on save (was only saving new fields)

#### FDF/XFDF Export
- **Forms Data Format Export** - ISO 32000-1:2008 Section 12.7.7
  - `FdfWriter` - Binary FDF export for form data exchange
  - `XfdfWriter` - XML XFDF export for web integration
  - `export_form_data_fdf()` / `export_form_data_xfdf()` on FormExtractor, DocumentEditor, Pdf
  - Hierarchical field representation in exports

### Added - Text Extraction Enhancements
- **TextChar Transformation** - Per-character positioning metadata (#27)
  - `origin` - Font baseline coordinates (x, y)
  - `rotation_degrees` - Character rotation angle
  - `matrix` - Full transformation matrix
  - Essential for pdfium-render migration

### Added - Image Metadata
- **DPI Calculation** - Resolution metadata for images
  - `horizontal_dpi` / `vertical_dpi` fields on `ImageContent`
  - `resolution()` - Get (h_dpi, v_dpi) tuple
  - `is_high_resolution()` / `is_low_resolution()` / `is_medium_resolution()` helpers
  - `calculate_dpi()` - Compute from pixel dimensions and bbox

### Added - Bounded Text Extraction
- **Spatial Filtering** - Extract text from rectangular regions
  - `RectFilterMode::Intersects` - Any overlap (default)
  - `RectFilterMode::FullyContained` - Completely within bounds
  - `RectFilterMode::MinOverlap(f32)` - Minimum overlap fraction
  - `TextSpanSpatial` trait - `intersects_rect()`, `contained_in_rect()`, `overlap_with_rect()`
  - `TextSpanFiltering` trait - `filter_by_rect()`, `extract_text_in_rect()`

### Added - Multimedia Annotations
- **MovieAnnotation** - Embedded video content
- **SoundAnnotation** - Audio content with playback controls
- **ScreenAnnotation** - Media renditions (video/audio players)
- **RichMediaAnnotation** - Flash/video rich media content

### Added - 3D Annotations
- **ThreeDAnnotation** - 3D model embedding
  - U3D and PRC format support
  - `ThreeDView` - Camera angles and lighting
  - `ThreeDAnimation` - Playback controls

### Added - Path Extraction
- **PathExtractor** - Vector graphics extraction
  - Lines, curves, rectangles, complex paths
  - Path transformation and bounding box calculation

### Added - XFA Form Support
- **XfaExtractor** - Extract XFA form data
- **XfaParser** - Parse XFA XML templates
- **XfaConverter** - Convert XFA forms to AcroForm

### Changed - Python Bindings
- **True Python 3.8-3.14 Support** - Fixed via `abi3-py38` (was only working on 3.11)
- **Modern Tooling** - uv, pdm, ruff integration
- **Code Quality** - All Python code formatted with ruff

### 🏆 Community Contributors

🥇 **@monchin** - Massive thanks for revolutionizing our Python ecosystem! Your PR #29 fixed a critical compatibility issue where PDFOxide only worked on Python 3.11 despite claiming 3.8+ support. By switching to `abi3-py38`, you enabled true cross-version compatibility (Python 3.8-3.14). The introduction of modern tooling (uv, pdm, ruff) brings PDFOxide's Python development to 2026 standards. This work directly enables thousands more Python developers to use PDFOxide. 💪🐍

🥈 **@bikallem** - Thanks for the thoughtful feature request (#27) comparing PDFOxide to pdfium-render. Your detailed analysis of missing origin coordinates and rotation angles led directly to our TextChar transformation feature. This makes PDFOxide a viable migration path for pdfium-render users. 🎯

## [0.3.0] - 2026-01-10
> Unified API, PDF Creation & Editing

### Added - Unified `Pdf` API
- **One API for Extract, Create, and Edit** - The new `Pdf` class unifies all PDF operations
  - `Pdf::open("input.pdf")` - Open existing PDF for reading and editing
  - `Pdf::from_markdown(content)` - Create new PDF from Markdown
  - `Pdf::from_html(content)` - Create new PDF from HTML
  - `Pdf::from_text(content)` - Create new PDF from plain text
  - `Pdf::from_image(path)` - Create PDF from image file
  - DOM-like page navigation with `pdf.page(0)` for querying and modifying content
  - Seamless save with `pdf.save("output.pdf")` or `pdf.save_encrypted()`
- **Fluent Builder Pattern** - `PdfBuilder` for advanced configuration
  ```rust
  PdfBuilder::new()
      .title("My Document")
      .author("Author Name")
      .page_size(PageSize::A4)
      .from_markdown("# Content")?
  ```

### Added - PDF Creation
- **PDF Creation API** - Fluent `DocumentBuilder` for programmatic PDF generation
  - `Pdf::create()` / `DocumentBuilder::new()` entry points
  - Page sizing (Letter, A4, custom dimensions)
  - Text rendering with Base14 fonts and styling
  - Image embedding (JPEG/PNG) with positioning
- **Table Rendering** - `TableRenderer` for styled tables
  - Headers, borders, cell spans, alternating row colors
  - Column width control (fixed, percentage, auto)
  - Cell alignment and padding
- **Graphics API** - Advanced visual effects
  - Colors (RGB, CMYK, grayscale)
  - Linear and radial gradients
  - Tiling patterns with presets
  - Blend modes and transparency (ExtGState)
- **Page Templates** - Reusable page elements
  - Headers and footers with placeholders
  - Page numbering formats
  - Watermarks (text-based)
- **Barcode Generation** (requires `barcodes` feature)
  - QR codes with configurable size and error correction
  - Code128, EAN-13, UPC-A, Code39, ITF barcodes
  - Customizable colors and dimensions

### Added - PDF Editing
- **Editor API** - DOM-like editing with round-trip preservation
  - `DocumentEditor` for modifying existing PDFs
  - Content addition without breaking existing structure
  - Resource management for fonts and images
- **Annotation Support** - Full read/write for all types
  - Text markup: highlights, underlines, strikeouts, squiggly
  - Notes: sticky notes, comments, popups
  - Shapes: rectangles, circles, lines, polygons, polylines
  - Drawing: ink/freehand annotations
  - Stamps: standard and custom stamps
  - Special: file attachments, redactions, carets
- **Form Fields** - Interactive form creation
  - Text fields (single/multiline, password, comb)
  - Checkboxes with custom appearance
  - Radio button groups
  - Dropdown and list boxes
  - Push buttons with actions
  - Form flattening (convert fields to static content)
- **Link Annotations** - Navigation support
  - External URLs
  - Internal page navigation
  - Styled link appearance
- **Outline Builder** - Bookmark/TOC creation
  - Hierarchical structure
  - Page destinations
  - Styling (bold, italic, colors)
- **PDF Layers** - Optional Content Groups (OCG)
  - Create and manage content layers
  - Layer visibility controls

### Added - PDF Compliance & Validation
- **PDF/A Validation** - ISO 19005 compliance checking
  - PDF/A-1a, PDF/A-1b (ISO 19005-1)
  - PDF/A-2a, PDF/A-2b, PDF/A-2u (ISO 19005-2)
  - PDF/A-3a, PDF/A-3b (ISO 19005-3)
- **PDF/A Conversion** - Convert documents to archival format
  - Automatic font embedding
  - XMP metadata injection
  - ICC color profile conversion
- **PDF/X Validation** - ISO 15930 print production compliance
  - PDF/X-1a:2001, PDF/X-1a:2003
  - PDF/X-3:2002, PDF/X-3:2003
  - PDF/X-4, PDF/X-4p
  - PDF/X-5g, PDF/X-5n, PDF/X-5pg
  - PDF/X-6, PDF/X-6n, PDF/X-6p
  - 40+ specific error codes for violations
- **PDF/UA Validation** - ISO 14289 accessibility compliance
  - Tagged PDF structure validation
  - Language specification checks
  - Alt text requirements
  - Heading hierarchy validation
  - Table header validation
  - Form field accessibility
  - Reading order verification

### Added - Security & Encryption
- **Encryption on Write** - Password-protect PDFs when saving
  - AES-256 (V=5, R=6) - Modern 256-bit encryption (default)
  - AES-128 (V=4, R=4) - Modern 128-bit encryption
  - RC4-128 (V=2, R=3) - Legacy 128-bit encryption
  - RC4-40 (V=1, R=2) - Legacy 40-bit encryption
  - `Pdf::save_encrypted()` for simple password protection
  - `Pdf::save_with_encryption()` for full configuration
- **Permission Controls** - Granular access restrictions
  - Print, copy, modify, annotate permissions
  - Form fill and accessibility extraction controls
- **Digital Signatures** (foundation, requires `signatures` feature)
  - ByteRange calculation for signature placeholders
  - PKCS#7/CMS signature structure support
  - X.509 certificate parsing
  - Signature verification framework

### Added - Document Features
- **Page Labels** - Custom page numbering
  - Roman numerals, letters, decimal formats
  - Prefix support (e.g., "A-1", "B-2")
  - `PageLabelsBuilder` for creation
  - Extract existing labels from documents
- **XMP Metadata** - Extensible metadata support
  - Dublin Core properties (title, creator, description)
  - PDF properties (producer, keywords)
  - Custom namespace support
  - Full read/write capability
- **Embedded Files** - File attachments
  - Attach files to PDF documents
  - MIME type and description support
  - Relationship specification (Source, Data, etc.)
- **Linearization** - Web-optimized PDFs
  - Fast web view support
  - Streaming delivery optimization

### Added - Search & Analysis
- **Text Search** - Pattern-based document search
  - Regex pattern support
  - Case-sensitive/insensitive options
  - Position tracking with page/coordinates
  - Whole word matching
- **Page Rendering** (requires `rendering` feature)
  - Render pages to PNG/JPEG images
  - Configurable DPI and scale
  - Pure Rust via tiny-skia (no external dependencies)
- **Debug Visualization** (requires `rendering` feature)
  - Visualize text bounding boxes
  - Element highlighting for debugging
  - Export annotated page images

### Added - Document Conversion
- **Office to PDF** (requires `office` feature)
  - **DOCX**: Word documents with paragraphs, headings, lists, formatting
  - **XLSX**: Excel spreadsheets via calamine (sheets, cells, tables)
  - **PPTX**: PowerPoint presentations (slides, titles, text boxes)
  - `OfficeConverter` with auto-detection
  - `OfficeConfig` for page size, margins, fonts
  - Python bindings: `OfficeConverter.from_docx()`, `from_xlsx()`, `from_pptx()`

### Added - Python Bindings
- `Pdf` class for PDF creation
- `Color`, `BlendMode`, `ExtGState` for graphics
- `LinearGradient`, `RadialGradient` for gradients
- `LineCap`, `LineJoin`, `PatternPresets` for styling
- `save_encrypted()` method with permission flags
- `OfficeConverter` class for Office document conversion

### Changed
- Description updated to "The Complete PDF Toolkit: extract, create, and edit PDFs"
- Python module docstring updated for v0.3.0 features
- Branding updated with Extract/Create/Edit pillars

### Fixed
- **Outline action handling** - correctly dereference actions indirectly referenced by outline items

### 🏆 Community Contributors

🥇 **@jvantuyl** - Thanks for the thorough PR #16 fixing outline action dereferencing! Your investigation uncovered that some PDFs embed actions directly while others use indirect references - a subtle PDF spec detail that was breaking bookmark navigation. Your fix included comprehensive tests ensuring this won't regress. 🔍✨

🙏 **@mert-kurttutan** - Thanks for the honest feedback in issue #15 about README clutter. Your perspective as a new user helped us realize we were overwhelming people with information. The resulting documentation cleanup makes PDFOxide more approachable. 📚

## [0.2.6] - 2026-01-09
> CJK Support & Structure Tree Enhancements

### Added
- **TagSuspect/MarkInfo support** (ISO 32000-1 Section 14.7.1)
  - Parse MarkInfo dictionary from document catalog (`marked`, `suspects`, `user_properties`)
  - `PdfDocument::mark_info()` method to retrieve MarkInfo
  - Automatic fallback to geometric ordering when structure tree is marked as suspect
- **Word Break /WB structure element** (Section 14.8.4.4)
  - Support for explicit word boundaries in CJK text
  - `StructType::WB` variant and `is_word_break()` helper
  - Word break markers emitted during structure tree traversal
- **Predefined CMap support for CJK fonts** (Section 9.7.5.2)
  - Adobe-GB1 (Simplified Chinese) - ~500 common character mappings
  - Adobe-Japan1 (Japanese) - Hiragana, Katakana, Kanji mappings
  - Adobe-CNS1 (Traditional Chinese) - Bopomofo and CJK mappings
  - Adobe-Korea1 (Korean) - Hangul and Hanja mappings
  - Fallback identity mapping for common Unicode ranges
- **Abbreviation expansion /E support** (Section 14.9.5)
  - Parse `/E` entry from marked content properties
  - `expansion` field on `StructElem` for structure-level abbreviations
- **Object reference resolution utility**
  - `PdfDocument::resolve_references()` for recursive reference handling in complex PDF structures
- **Type 0 /W array parsing** for CIDFont glyph widths
  - Proper spacing for CJK text using CIDFont width specifications
- **ActualText verification tests** - comprehensive test coverage for PDF Spec Section 14.9.4

### Fixed
- **Soft hyphen handling** (U+00AD) - now correctly treated as valid continuation hyphen for word reconstruction

### Changed
- **Enhanced artifact filtering** with subtype support
  - `ArtifactType::Pagination` with subtypes: Header, Footer, Watermark, PageNumber
  - `ArtifactType::Layout` and `ArtifactType::Background` classification
- `OrderedContent.mcid` changed to `Option<u32>` to support word break markers

## [0.2.5] - 2026-01-09
> Image Embedding & Export

### Added
- **Image embedding**: Both HTML and Markdown now support embedded base64 images when `embed_images=true` (default)
  - HTML: `<img src="data:image/png;base64,...">`
  - Markdown: `![alt](data:image/png;base64,...)` (works in Obsidian, Typora, VS Code, Jupyter)
- **Image file export**: Set `embed_images=false` + `image_output_dir` to save images as files with relative path references
- New `embed_images` option in `ConversionOptions` to control embedding behavior
- `PdfImage::to_base64_data_uri()` method for converting images to data URIs
- `PdfImage::to_png_bytes()` method for in-memory PNG encoding
- Python bindings: new `embed_images` parameter for `to_html`, `to_markdown`, and `*_all` methods

## [0.2.4] - 2026-01-09
> CTM Fix & Formula Rendering

### Fixed
- CTM (Current Transformation Matrix) now correctly applied to text positions per PDF Spec ISO 32000-1:2008 Section 9.4.4 (#11)

### Added
- Structure tree: `/Alt` (alternate description) parsing for accessibility text on formulas and figures
- Structure tree: `/Pg` (page reference) resolution - correctly maps structure elements to page numbers
- `FormulaRenderer` module for extracting formula regions as base64 images from rendered pages
- `ConversionOptions`: new fields `render_formulas`, `page_images`, `page_dimensions` for formula image embedding
- Regression tests for CTM transformation

### 🏆 Community Contributors

🐛➡️✅ **@mert-kurttutan** - Thanks for the detailed bug report (#11) with reproducible sample PDF! Your report exposed a fundamental CTM transformation bug affecting text positioning across the entire library. This fix was critical for production use. 🎉

## [0.2.3] - 2026-01-07
> BT/ET Matrix Reset & Text Processing

### Fixed
- BT/ET matrix reset per PDF spec Section 9.4.1 (PR #10 by @drahnr)
- Geometric spacing detection in markdown converter (#5)
- Verbose extractor logs changed from info to trace (#7)
- docs.rs build failure (excluded tesseract-rs)

### Added
- `apply_intelligent_text_processing()` method for ligature expansion, hyphenation reconstruction, and OCR cleanup (#6)

### Changed
- Removed unused tesseract-rs dependency

### 🏆 Community Contributors

🥇 **@drahnr** - Huge thanks for PR #10 fixing the BT/ET matrix reset issue! This was a subtle PDF spec compliance bug (Section 9.4.1) where text matrices weren't being reset between text blocks, causing positions to accumulate and become unusable. Your fix restored correct text positioning for all PDFs. 💪📐

🔬 **@JanIvarMoldekleiv** - Thanks for the detailed bug report (#5) about missing spaces and lost table structure! Your analysis even identified the root cause in the code - the markdown converter wasn't using geometric spacing analysis. This level of investigation made the fix straightforward. 🕵️‍♂️

🎯 **@Borderliner** - Thanks for two important catches! Issue #6 revealed that `apply_intelligent_text_processing()` was documented but not actually available (oops! 😅), and #7 caught our overly verbose INFO-level logging flooding terminals. Both fixed immediately! 🔧

## [0.2.2] - 2025-12-15
> Discoverability Improvements

### Changed
- Optimized crate keywords for better discoverability

## [0.2.1] - 2025-12-15
> Encrypted PDF Fixes

### Fixed
- Encrypted stream decoding improvements (#3)
- CI/CD pipeline fixes

### 🏆 Community Contributors

🥇 **@threebeanbags** - Huge thanks for PRs #2 and #3 fixing encrypted PDF support! 🔐 Your first PR identified that decryption needed to happen before decompression - a critical ordering issue. Your follow-up PR #3 went deeper, fixing encryption handler initialization timing and adding Form XObject encryption support. These fixes made PDFOxide actually work with password-protected PDFs in production. 💪🎉

## [0.1.4] - 2025-12-12

### Fixed
- Encrypted stream decoding (#2)
- Documentation and doctest fixes

## [0.1.3] - 2025-12-12

### Fixed
- Encrypted stream decoding refinements

## [0.1.2] - 2025-11-27

### Added
- Python 3.13 support
- GitHub sponsor configuration

## [0.1.1] - 2025-11-26

### Added
- Cross-platform binary builds (Linux, macOS, Windows)

## [0.1.0] - 2025-11-06

### Added
- Initial release
- PDF text extraction with spec-compliant Unicode mapping
- Intelligent reading order detection
- Python bindings via PyO3
- Support for encrypted PDFs
- Form field extraction
- Image extraction

### 🌟 Early Adopters

💖 **@magnus-trent** - Thanks for issue #1, our first community feedback! Your message that PDFOxide "unlocked an entire pipeline" you'd been working on for a month validated that we were solving real problems. Early encouragement like this keeps open source projects going. 🚀
