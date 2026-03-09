fn main() {
    // Make memory.x available to the linker
    println!(
        "cargo:rustc-link-search={}",
        std::env::var("CARGO_MANIFEST_DIR").unwrap()
    );
    println!("cargo:rerun-if-changed=memory.x");

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");

    #[cfg(feature = "ble-midi")]
    download_cyw43_firmware();
}

#[cfg(feature = "ble-midi")]
fn download_cyw43_firmware() {
    use std::path::Path;

    let fw_dir = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("cyw43-firmware");
    std::fs::create_dir_all(&fw_dir).unwrap();

    let base_url = "https://github.com/embassy-rs/embassy/raw/refs/heads/main/cyw43-firmware/";
    let files = [
        "43439A0.bin",
        "43439A0_btfw.bin",
        "43439A0_clm.bin",
    ];

    let client = reqwest::blocking::Client::new();

    for file in &files {
        let dest = fw_dir.join(file);
        if dest.exists() {
            continue;
        }
        println!("cargo:warning=Downloading CYW43 firmware: {}", file);
        let url = format!("{}{}", base_url, file);
        let resp = client.get(&url).send().unwrap_or_else(|e| {
            panic!("Failed to download {}: {}", file, e);
        });
        let bytes = resp.bytes().unwrap();
        std::fs::write(&dest, &bytes).unwrap();
    }
}
