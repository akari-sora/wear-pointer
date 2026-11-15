fn main() {
    println!("cargo:rerun-if-changed=Info.plist");
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon-64.png");
    println!("cargo:rerun-if-changed=assets/tray-template-44.png");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        let plist = std::path::Path::new(&manifest_dir).join("Info.plist");
        for bin in ["wear-pointer", "wear-pointer-app"] {
            println!(
                "cargo:rustc-link-arg-bin={bin}=-Wl,-sectcreate,__TEXT,__info_plist,{}",
                plist.display()
            );
        }
    } else if target_os == "windows" {
        embed_resource::compile("assets/app.rc", embed_resource::NONE);
    }
}
