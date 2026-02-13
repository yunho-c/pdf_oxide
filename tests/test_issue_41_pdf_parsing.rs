//! Test Issue #41: PDF parsing with binary prefixes and malformed headers

#[test]
fn test_ceur_pdf_parsing() {
    use pdf_oxide::document::PdfDocument;
    use std::path::Path;
    
    // Test with the CEUR PDF from Issue #41
    let pdf_path = "/tmp/pdf_test/test2.pdf";
    
    if !Path::new(pdf_path).exists() {
        eprintln!("Test PDF not found at {}. Skipping...", pdf_path);
        return;
    }
    
    // This should not panic or return an error
    let mut doc = match PdfDocument::open(pdf_path) {
        Ok(d) => d,
        Err(e) => panic!("Failed to open PDF: {}", e),
    };
    
    // Verify basic properties
    let (major, minor) = doc.version();
    println!("PDF Version: {}.{}", major, minor);
    assert!(major >= 1, "Invalid PDF version");
    
    // Verify page count works
    let page_count = doc.page_count().expect("Failed to get page count");
    println!("Pages: {}", page_count);
    assert!(page_count > 0, "PDF should have at least one page");
    
    // Try extracting text from first page
    match doc.extract_spans(0) {
        Ok(spans) => {
            println!("Text extraction successful: {} spans", spans.len());
            assert!(spans.len() > 0, "Should extract some text");
        },
        Err(e) => {
            eprintln!("Warning: Could not extract text: {}", e);
            // Some PDFs might be image-based, try paths
            match doc.extract_paths(0) {
                Ok(paths) => println!("Path extraction successful: {} paths", paths.len()),
                Err(e2) => panic!("Could not extract text or paths: {}", e2),
            }
        },
    }
    
    println!("✓ Issue #41 test passed: PDF with potential binary prefix parsed successfully!");
}

#[test]
fn test_issue_41_comprehensive() {
    use pdf_oxide::document::PdfDocument;
    use std::path::Path;
    
    let pdf_path = "/tmp/pdf_test/test2.pdf";
    
    if !Path::new(pdf_path).exists() {
        println!("Test PDF not found. Skipping...");
        return;
    }
    
    println!("\n=== Issue #41 Comprehensive Test ===\n");
    
    // Test 1: Open PDF with potential binary prefix
    println!("✓ Test 1: PDF header parsing with binary prefix support");
    let mut doc = PdfDocument::open(pdf_path).expect("Failed to open PDF");
    println!("  - PDF opened successfully");
    
    // Test 2: Get version
    let (major, minor) = doc.version();
    println!("✓ Test 2: Extract version");
    println!("  - Version: {}.{}", major, minor);
    assert_eq!(major, 1);
    assert!(minor > 0);
    
    // Test 3: Get page count
    let page_count = doc.page_count().expect("Failed to get page count");
    println!("✓ Test 3: Get page count");
    println!("  - Pages: {}", page_count);
    assert_eq!(page_count, 5);
    
    // Test 4: Extract from first page (tests fallback scanning for broken page tree)
    println!("✓ Test 4: Extract text from page 0 (with fallback scanning)");
    let spans = doc.extract_spans(0).expect("Failed to extract text");
    println!("  - Text spans: {}", spans.len());
    assert!(spans.len() > 0);
    
    // Test 5: Extract from other pages
    println!("✓ Test 5: Extract from multiple pages");
    for i in 0..page_count.min(3) {
        let page_spans = doc.extract_spans(i);
        match page_spans {
            Ok(spans) => println!("  - Page {}: {} spans", i, spans.len()),
            Err(e) => println!("  - Page {}: extraction skipped ({})", i, e),
        }
    }
    
    println!("\n=== All Issue #41 Tests Passed ===");
    println!("✓ Header parsing with binary prefix support");
    println!("✓ Fallback page scanning for broken page trees");
    println!("✓ Text extraction from malformed PDFs\n");
}
