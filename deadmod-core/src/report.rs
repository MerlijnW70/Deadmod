//! Output formatting - plaintext and JSON.

use serde_json::json;

/// Prints dead modules in plain text format.
pub fn print_plain(dead: &[&str]) {
    if dead.is_empty() {
        println!("No dead modules found.");
    } else {
        println!("DEAD MODULES ({}):", dead.len());
        for m in dead {
            println!("- {}", m);
        }
    }
}

/// Prints dead modules in JSON format.
///
/// Falls back to simple format if serialization fails (should never happen
/// with string arrays, but NASA-grade means handling all cases).
pub fn print_json(dead: &[&str]) {
    match serde_json::to_string_pretty(&json!({ "dead": dead })) {
        Ok(json) => println!("{}", json),
        Err(e) => {
            // Fallback: output in a simpler format
            eprintln!("[WARN] JSON serialization failed: {}", e);
            println!("{{\"dead\": {:?}}}", dead);
        }
    }
}
