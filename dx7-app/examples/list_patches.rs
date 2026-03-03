//! List ALL available DX7 patches across all sysex banks.
//! Run: cargo run --example list_patches
//!
//! Scans sysex/factory/ and sysex/vrc/ directories and prints every
//! patch name from every bank file, useful for finding better GM assignments.

use dx7_core::patch::DxVoice;
use std::fs;
use std::path::Path;

fn list_bank(sysex_dir: &str, rel_path: &str) {
    let full_path = format!("{}/{}", sysex_dir, rel_path);
    let data = match fs::read(&full_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  ERROR reading {}: {}", full_path, e);
            return;
        }
    };
    let voices = match DxVoice::parse_bulk_dump(&data) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("  ERROR parsing {}: {}", full_path, e);
            return;
        }
    };
    println!("=== {} ({} patches) ===", rel_path, voices.len());
    for (i, voice) in voices.iter().enumerate() {
        let name = std::str::from_utf8(&voice.name)
            .unwrap_or("??????????")
            .trim_end();
        println!("  {:>2}: {}", i, name);
    }
    println!();
}

fn collect_syx_files(dir: &Path) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "syx") {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    files.push(name.to_string());
                }
            }
        }
    }
    files.sort();
    files
}

fn main() {
    let sysex_dir = "sysex";

    // --- Factory ROMs ---
    println!("###############################################");
    println!("###         FACTORY ROM BANKS               ###");
    println!("###############################################");
    println!();

    let factory_dir = Path::new(sysex_dir).join("factory");
    let factory_files = collect_syx_files(&factory_dir);
    for file in &factory_files {
        list_bank(sysex_dir, &format!("factory/{}", file));
    }

    // --- VRC Cartridges ---
    println!("###############################################");
    println!("###         VRC CARTRIDGE BANKS             ###");
    println!("###############################################");
    println!();

    let vrc_dir = Path::new(sysex_dir).join("vrc");
    let vrc_files = collect_syx_files(&vrc_dir);
    for file in &vrc_files {
        list_bank(sysex_dir, &format!("vrc/{}", file));
    }

    // --- Summary ---
    let total_banks = factory_files.len() + vrc_files.len();
    let total_patches = total_banks * 32; // DX7 bulk dump is always 32 voices
    println!("###############################################");
    println!("### SUMMARY: {} banks, ~{} patches total", total_banks, total_patches);
    println!("###############################################");
}
