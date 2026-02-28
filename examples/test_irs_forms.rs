//! Focused test: fill IRS W-2 with realistic values, save incremental, reopen,
//! and dump extract_text / to_markdown output for visual confirmation.
use pdf_oxide::PdfDocument;
use pdf_oxide::converters::ConversionOptions;
use pdf_oxide::editor::{DocumentEditor, EditableDocument, SaveOptions};
use pdf_oxide::editor::form_fields::FormFieldValue;
use pdf_oxide::extractors::forms::FormExtractor;

fn test_fill_and_verify(path: &str, label: &str) {
    println!("\n{}", "=".repeat(70));
    println!("  {}: Fill realistic values, save incremental, verify", label);
    println!("{}\n", "=".repeat(70));

    // Step 1: Show unfilled extract_text page 1 (CopyA)
    println!("--- UNFILLED: extract_text page 1 (first 500 chars) ---");
    let mut doc = match PdfDocument::open(path) {
        Ok(d) => d,
        Err(e) => {
            println!("  SKIP: Cannot open {}: {}", path, e);
            return;
        }
    };
    let unfilled_text = doc.extract_text(1).unwrap_or_else(|e| format!("ERR: {}", e));
    let preview_len = unfilled_text.len().min(500);
    println!("{}\n", &unfilled_text[..preview_len]);

    // Step 2: Fill with realistic W-2 values
    let mut editor = DocumentEditor::open(path).expect("open editor");

    let fills: Vec<(&str, FormFieldValue, &str)> = vec![
        ("topmostSubform[0].CopyA[0].BoxA_ReadOrder[0].f1_01[0]",
         FormFieldValue::Text("999-88-7777".into()), "SSN"),
        ("topmostSubform[0].CopyA[0].Col_Left[0].f1_02[0]",
         FormFieldValue::Text("12-3456789".into()), "EIN"),
        ("topmostSubform[0].CopyA[0].Col_Left[0].f1_03[0]",
         FormFieldValue::Text("Acme Corporation\n123 Main St\nAnytown, ST 12345".into()), "Employer"),
        ("topmostSubform[0].CopyA[0].Col_Left[0].FirstName_ReadOrder[0].f1_05[0]",
         FormFieldValue::Text("John".into()), "First name"),
        ("topmostSubform[0].CopyA[0].Col_Left[0].LastName_ReadOrder[0].f1_06[0]",
         FormFieldValue::Text("Smith".into()), "Last name"),
        ("topmostSubform[0].CopyA[0].Col_Right[0].Box1_ReadOrder[0].f1_09[0]",
         FormFieldValue::Text("85000.00".into()), "Wages"),
        ("topmostSubform[0].CopyA[0].Col_Right[0].f1_10[0]",
         FormFieldValue::Text("15000.00".into()), "Fed tax"),
        ("topmostSubform[0].CopyA[0].Col_Right[0].Retirement_ReadOrder[0].c1_3[0]",
         FormFieldValue::Boolean(true), "Retirement checkbox"),
    ];

    println!("--- FILLING fields ---");
    for (name, val, lbl) in &fills {
        match editor.set_form_field_value(name, val.clone()) {
            Ok(()) => println!("  OK: {} = {:?}", lbl, val),
            Err(e) => println!("  FAIL: {} => {}", lbl, e),
        }
    }

    let tmp = format!("/tmp/irs_filled_{}.pdf", label);
    editor.save_with_options(&tmp, SaveOptions::incremental()).expect("save");
    println!("  Saved: {}\n", tmp);

    // Step 3: Reopen and verify with FormExtractor
    let mut filled = PdfDocument::open(&tmp).expect("reopen");

    println!("--- REOPEN: FormExtractor ---");
    let fields = FormExtractor::extract_fields(&mut filled).expect("extract");
    for (name, _expected, lbl) in &fills {
        if let Some(f) = fields.iter().find(|f| f.full_name == *name) {
            println!("  {}: {:?}", lbl, f.value);
        } else {
            println!("  {}: NOT FOUND!", lbl);
        }
    }

    // Step 4: extract_text on page 1 — dump full output
    println!("\n--- REOPEN: extract_text page 1 (FULL) ---");
    let filled_text = filled.extract_text(1).unwrap_or_else(|e| format!("ERR: {}", e));
    println!("{}", filled_text);

    // Step 5: Check each value in the text
    println!("\n--- VALUE CHECK in extract_text ---");
    let value_checks: Vec<(&str, &str)> = vec![
        ("999-88-7777", "SSN"),
        ("12-3456789", "EIN"),
        ("Acme Corporation", "Employer"),
        ("John", "First name"),
        ("Smith", "Last name"),
        ("85000.00", "Wages"),
        ("15000.00", "Fed tax"),
        ("[x]", "Retirement checked"),
        ("[ ]", "Other checkbox unchecked"),
    ];
    for (val, lbl) in &value_checks {
        if filled_text.contains(val) {
            println!("  PASS: '{}' found ({})", val, lbl);
        } else {
            println!("  FAIL: '{}' NOT found ({})", val, lbl);
        }
    }

    // Step 6: to_markdown page 1
    println!("\n--- REOPEN: to_markdown page 1 (include_form_fields=true) ---");
    let opts = ConversionOptions { include_form_fields: true, ..Default::default() };
    let filled_md = filled.to_markdown(1, &opts).unwrap_or_else(|e| format!("ERR: {}", e));
    println!("{}", filled_md);

    println!("\n--- VALUE CHECK in to_markdown ---");
    for (val, lbl) in &value_checks {
        if filled_md.contains(val) {
            println!("  PASS: '{}' found ({})", val, lbl);
        } else {
            println!("  FAIL: '{}' NOT found ({})", val, lbl);
        }
    }

    // Step 7: to_markdown with form fields OFF
    println!("\n--- to_markdown include_form_fields=false ---");
    let opts_off = ConversionOptions { include_form_fields: false, ..Default::default() };
    let md_off = filled.to_markdown(1, &opts_off).unwrap_or_else(|e| format!("ERR: {}", e));
    let any_leak = value_checks.iter().any(|(v, _)| {
        *v != "[ ]" && *v != "[x]" && md_off.contains(v)
    });
    if any_leak {
        println!("  FAIL: Values leaked with include_form_fields=false");
    } else {
        println!("  PASS: No filled values leak");
    }
}

fn main() {
    let irs_dir = "/home/yfedoseev/projects/pdf_oxide_tests/irs";

    test_fill_and_verify(&format!("{}/fw2.pdf", irs_dir), "fw2");
    test_fill_and_verify(&format!("{}/fw2_2024.pdf", irs_dir), "fw2_2024");
}
