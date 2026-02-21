//! Per-page timing benchmark for slow PDFs.
//!
//! Reports per-PDF and per-page timing to identify performance bottlenecks.
//!
//! Usage:
//!   cargo run --release --example bench_slow_pdfs -- <directory>

use pdf_oxide::document::PdfDocument;
use std::env;
use std::fs;
use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = env::args().collect();
    let dir = if args.len() > 1 {
        &args[1]
    } else {
        eprintln!("Usage: bench_slow_pdfs <directory>");
        std::process::exit(1);
    };

    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("failed to read directory")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext.eq_ignore_ascii_case("pdf"))
                .unwrap_or(false)
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    eprintln!("Found {} PDFs in {}", entries.len(), dir);
    println!("pdf,pages,total_ms,avg_ms_per_page,max_page_ms,max_page_idx,chars,error");

    for entry in &entries {
        let path = entry.path();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();

        let start = Instant::now();
        let mut doc = match PdfDocument::open(&path) {
            Ok(d) => d,
            Err(e) => {
                println!("{},0,0,0,0,0,0,{}", filename, e);
                continue;
            }
        };

        // Try common passwords
        for pw in &[b"" as &[u8], b"owner", b"user", b"password"] {
            let _ = doc.authenticate(pw);
        }

        let page_count = match doc.page_count() {
            Ok(n) => n,
            Err(e) => {
                println!("{},0,0,0,0,0,0,{}", filename, e);
                continue;
            }
        };

        let mut total_chars = 0usize;
        let mut max_page_ms = 0.0f64;
        let mut max_page_idx = 0usize;
        let mut slow_pages = Vec::new();

        for i in 0..page_count {
            let page_start = Instant::now();
            match doc.extract_text(i) {
                Ok(text) => {
                    let page_ms = page_start.elapsed().as_secs_f64() * 1000.0;
                    total_chars += text.len();
                    if page_ms > max_page_ms {
                        max_page_ms = page_ms;
                        max_page_idx = i;
                    }
                    if page_ms > 1000.0 {
                        slow_pages.push((i, page_ms, text.len()));
                    }
                }
                Err(e) => {
                    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
                    println!(
                        "{},{},{:.0},{:.0},{:.0},{},{},page {}: {}",
                        filename, page_count, total_ms, 0, 0, 0, total_chars, i, e
                    );
                    continue;
                }
            }
        }

        let total_ms = start.elapsed().as_secs_f64() * 1000.0;
        let avg_ms = if page_count > 0 {
            total_ms / page_count as f64
        } else {
            0.0
        };

        println!(
            "{},{},{:.0},{:.1},{:.0},{},{},",
            filename, page_count, total_ms, avg_ms, max_page_ms, max_page_idx, total_chars
        );

        // Print slow pages to stderr for investigation
        if !slow_pages.is_empty() {
            eprintln!(
                "  SLOW: {} ({} pages, {:.1}s total, {:.1}ms avg)",
                &filename[..filename.len().min(60)],
                page_count,
                total_ms / 1000.0,
                avg_ms
            );
            for (page, ms, chars) in &slow_pages {
                eprintln!("    page {:>4}: {:>8.0}ms  {:>6} chars", page, ms, chars);
            }
        }
    }
}
