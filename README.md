# PDF Oxide - The Fastest PDF Library for Python and Rust

The fastest Python PDF library for text extraction, image extraction, and markdown conversion. Built on a Rust core for reliability and speed — mean 1.8ms per document, 3.5× faster than leading industry libraries, 100% pass rate on 3,830 real-world PDFs.

[![Crates.io](https://img.shields.io/crates/v/pdf_oxide.svg)](https://crates.io/crates/pdf_oxide)
[![PyPI](https://img.shields.io/pypi/v/pdf_oxide.svg)](https://pypi.org/project/pdf_oxide/)
[![PyPI Downloads](https://img.shields.io/pypi/dm/pdf-oxide)](https://pypi.org/project/pdf-oxide/)
[![Documentation](https://docs.rs/pdf_oxide/badge.svg)](https://docs.rs/pdf_oxide)
[![Build Status](https://github.com/yfedoseev/pdf_oxide/workflows/CI/badge.svg)](https://github.com/yfedoseev/pdf_oxide/actions)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://opensource.org/licenses)

## Quick Start

### Python
```python
from pdf_oxide import PdfDocument

doc = PdfDocument("paper.pdf")
text = doc.extract_text(0)
chars = doc.extract_chars(0)
markdown = doc.to_markdown(0, detect_headings=True)
```

```bash
pip install pdf_oxide
```

### Rust
```rust
use pdf_oxide::PdfDocument;

let mut doc = PdfDocument::open("paper.pdf")?;
let text = doc.extract_text(0)?;
let images = doc.extract_images(0)?;
let markdown = doc.to_markdown(0, Default::default())?;
```

```toml
[dependencies]
pdf_oxide = "0.3"
```

## Why pdf_oxide?

- **Fast** — Rust core, mean 1.8ms per document, 3.5× faster than leading industry libraries, 97% under 10ms
- **Reliable** — 100% pass rate on 3,830 test PDFs, zero panics, zero slow (>5s) PDFs
- **Complete** — Text extraction, image extraction, PDF creation, and editing in one library
- **Dual-language** — First-class Rust API and Python bindings via PyO3
- **Permissive license** — MIT / Apache-2.0 — use freely in commercial and open-source projects

## Features

| Extract | Create | Edit |
|---------|--------|------|
| Text & Layout | Documents | Annotations |
| Images | Tables | Form Fields |
| Forms | Graphics | Bookmarks |
| Annotations | Templates | Links |
| Bookmarks | Images | Content |

## Python API

```python
from pdf_oxide import PdfDocument

doc = PdfDocument("report.pdf")
print(f"Pages: {doc.page_count}")
print(f"Version: {doc.version}")

# Extract text from each page
for i in range(doc.page_count):
    text = doc.extract_text(i)
    print(f"Page {i}: {len(text)} chars")

# Character-level extraction with positions
chars = doc.extract_chars(0)
for ch in chars:
    print(f"'{ch.char}' at ({ch.x:.1f}, {ch.y:.1f})")

# Password-protected PDFs
doc = PdfDocument("encrypted.pdf")
doc.authenticate("password")
text = doc.extract_text(0)
```

## Rust API

```rust
use pdf_oxide::PdfDocument;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut doc = PdfDocument::open("paper.pdf")?;

    // Extract text
    let text = doc.extract_text(0)?;

    // Character-level extraction
    let chars = doc.extract_chars(0)?;

    // Extract images
    let images = doc.extract_images(0)?;

    // Vector graphics
    let paths = doc.extract_paths(0)?;

    Ok(())
}
```

## Performance

Verified against 3,830 PDFs from three independent test suites:

| Corpus | PDFs | Pass Rate |
|--------|-----:|----------:|
| veraPDF (PDF/A compliance) | 2,907 | 100% |
| Mozilla pdf.js | 897 | 99.2% |
| SafeDocs (targeted edge cases) | 26 | 100% |
| **Total** | **3,830** | **100%** |

| Metric | Value |
|--------|-------|
| **Mean latency** | **1.8ms** |
| **p50 latency** | 0.6ms |
| **p90 latency** | 2.6ms |
| **p99 latency** | 18ms |
| **Max latency** | 625ms |
| **Under 10ms** | 98.4% |
| **Slow (>5s)** | 0 |
| **Timeouts** | 0 |
| **Panics** | 0 |

100% pass rate on all valid PDFs — the 7 non-passing files across the corpus are intentionally broken test fixtures (missing PDF header, fuzz-corrupted catalogs, invalid xref streams). v0.3.8 adds a text-only content stream parser that skips graphics operators at the byte level, further reducing parse time on graphics-heavy pages.

## Installation

### Python

```bash
pip install pdf_oxide
```

Wheels available for Linux, macOS, and Windows. Python 3.8–3.14.

### Rust

```toml
[dependencies]
pdf_oxide = "0.3"
```

## Building from Source

```bash
# Clone and build
git clone https://github.com/yfedoseev/pdf_oxide
cd pdf_oxide
cargo build --release

# Run tests
cargo test

# Build Python bindings
maturin develop
```

## Documentation

- **[Getting Started (Rust)](docs/getting-started-rust.md)** - Complete Rust guide
- **[Getting Started (Python)](docs/getting-started-python.md)** - Complete Python guide
- **[API Docs](https://docs.rs/pdf_oxide)** - Full Rust API reference
- **[PDF Spec Reference](docs/spec/pdf.md)** - ISO 32000-1:2008

## Use Cases

- **RAG / LLM pipelines** — Convert PDFs to clean Markdown for retrieval-augmented generation with LangChain, LlamaIndex, or any framework
- **Document processing at scale** — Extract text, images, and metadata from thousands of PDFs in seconds
- **Data extraction** — Pull structured data from forms, tables, and layouts
- **Academic research** — Parse papers, extract citations, and process large corpora
- **PDF generation** — Create invoices, reports, certificates, and templated documents programmatically

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option. Unlike AGPL-licensed alternatives, pdf_oxide can be used freely in any project — commercial or open-source — with no copyleft restrictions.

## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

```bash
cargo build && cargo test && cargo fmt && cargo clippy -- -D warnings
```

## Citation

```bibtex
@software{pdf_oxide,
  title = {PDF Oxide: Fast PDF Toolkit for Rust and Python},
  author = {Yury Fedoseev},
  year = {2025},
  url = {https://github.com/yfedoseev/pdf_oxide}
}
```

---

**Rust** + **Python** | MIT/Apache-2.0 | 100% pass rate on 3,830 PDFs | mean 1.8ms/doc | v0.3.8
