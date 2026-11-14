fn main() {
    println!("cargo:rerun-if-changed=Info.plist");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        let plist = std::path::Path::new(&manifest_dir).join("Info.plist");
        println!(
            "cargo:rustc-link-arg-bin=wear-pointer=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
}
