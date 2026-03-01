#!/bin/bash
# Extracts release notes for a given version from CHANGELOG.md
# Usage: extract-release-notes.sh <version>
# Outputs:
#   release-title.txt  — "v0.3.5 | Performance, ..."
#   release-notes.md   — Full body (changelog section + installation footer)

set -euo pipefail

VERSION="$1"
CHANGELOG="CHANGELOG.md"

if [ ! -f "$CHANGELOG" ]; then
  echo "Error: $CHANGELOG not found" >&2
  exit 1
fi

# Extract subtitle from "> ..." line after version header
SUBTITLE=$(awk "/^## \[${VERSION}\]/{found=1; next} found && /^>/{gsub(/^> */, \"\"); print; exit}" "$CHANGELOG")

# Build title
if [ -n "$SUBTITLE" ]; then
  echo "v${VERSION} | ${SUBTITLE}" > release-title.txt
else
  echo "v${VERSION}" > release-title.txt
fi

# Extract body: everything between this version's ## and the next ##
awk "/^## \[${VERSION}\]/{flag=1; next} /^## \[/{flag=0} flag" "$CHANGELOG" \
  | sed '/^> /d' \
  | sed '1{/^$/d}' > changelog-section.md

if [ ! -s changelog-section.md ]; then
  echo "Warning: No changelog content found for version ${VERSION}" >&2
fi

# Build release body = changelog section + installation footer
cat changelog-section.md > release-notes.md
cat >> release-notes.md << 'FOOTER'

---

### Installation

**Rust (crates.io)**
```bash
cargo add pdf_oxide
```

**Python (PyPI)**
```bash
pip install pdf_oxide
```

**JavaScript/WASM (npm)**
```bash
npm install pdf-oxide-wasm
```

**CLI (Homebrew)**
```bash
brew install yfedoseev/tap/pdf-oxide
```

**CLI (Scoop — Windows)**
```powershell
scoop bucket add pdf-oxide https://github.com/yfedoseev/scoop-pdf-oxide
scoop install pdf-oxide
```

**CLI (Shell installer)**
```bash
curl -fsSL https://raw.githubusercontent.com/yfedoseev/pdf_oxide/main/install.sh | sh
```

**CLI (cargo-binstall)**
```bash
cargo binstall pdf_oxide_cli
```

**MCP Server (for AI assistants)**
```bash
cargo install pdf_oxide_mcp
```

**Pre-built Binaries**
Download archives for Linux, macOS, and Windows from the assets below. Each archive includes both `pdf-oxide` (CLI) and `pdf-oxide-mcp` (MCP server).

### Platform Support
| Platform | Architecture | Archive |
|----------|-------------|---------|
| Linux | x86_64 (glibc) | `pdf_oxide-linux-x86_64-*.tar.gz` |
| Linux | x86_64 (musl) | `pdf_oxide-linux-x86_64-musl-*.tar.gz` |
| Linux | ARM64 | `pdf_oxide-linux-aarch64-*.tar.gz` |
| macOS | x86_64 (Intel) | `pdf_oxide-macos-x86_64-*.tar.gz` |
| macOS | ARM64 (Apple Silicon) | `pdf_oxide-macos-aarch64-*.tar.gz` |
| Windows | x86_64 | `pdf_oxide-windows-x86_64-*.zip` |

### Changelog
See [CHANGELOG.md](https://github.com/yfedoseev/pdf_oxide/blob/main/CHANGELOG.md) for full details.
FOOTER

# Cleanup
rm -f changelog-section.md

echo "Generated release-title.txt and release-notes.md for v${VERSION}"
