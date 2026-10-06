fn main() {
    macos_notifications();
    println!("cargo:rerun-if-env-changed=TELEAF_APP_API_ID");
    println!("cargo:rerun-if-env-changed=TELEAF_APP_API_HASH");
    let id = std::env::var("TELEAF_APP_API_ID").unwrap_or_default();
    let hash = std::env::var("TELEAF_APP_API_HASH").unwrap_or_default();
    let id = id.trim();
    let hash = hash.trim();
    if id.is_empty() && hash.is_empty() {
        return;
    }
    assert!(
        id.parse::<i32>().is_ok_and(|id| id > 0)
            && hash.len() == 32
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "TELEAF_APP_API_ID / TELEAF_APP_API_HASH must be a complete valid application credential pair"
    );
    println!("cargo:rustc-env=TELEAF_APP_API_ID={id}");
    println!("cargo:rustc-env=TELEAF_APP_API_HASH={hash}");
}

fn macos_notifications() {
    use std::path::PathBuf;
    use std::process::Command;

    println!("cargo:rerun-if-changed=packaging/macos-notifications/main.swift");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    if !std::env::var("HOST")
        .unwrap_or_default()
        .contains("apple-darwin")
    {
        println!(
            "cargo:warning=Build the macOS notification helper on a macOS host before packaging"
        );
        return;
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let profile = out.ancestors().nth(3).unwrap();
    let bundle = profile.join("Teleaf Notifications.app");
    let contents = bundle.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    let arch = if std::env::var("TARGET").unwrap().starts_with("aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    let status = Command::new("xcrun")
        .args(["swiftc", "-Osize", "-target"])
        .arg(format!("{arch}-apple-macosx15.0"))
        .arg("-module-cache-path")
        .arg(out.join("swift-cache"))
        .arg("packaging/macos-notifications/main.swift")
        .arg("-o")
        .arg(contents.join("MacOS/teleaf-notifications"))
        .status()
        .expect("macOS builds require Xcode Command Line Tools (swiftc)");
    assert!(
        status.success(),
        "Failed to build the macOS notification helper"
    );
    std::fs::write(contents.join("Info.plist"), format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>org.teleaf.notifications</string>
<key>CFBundleName</key><string>Teleaf Notifications</string>
<key>CFBundleDisplayName</key><string>Teleaf Notifications</string>
<key>CFBundleExecutable</key><string>teleaf-notifications</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{}</string>
<key>CFBundleVersion</key><string>{}</string>
<key>LSMinimumSystemVersion</key><string>15.0</string>
<key>LSUIElement</key><true/>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
"#, std::env::var("CARGO_PKG_VERSION").unwrap(), std::env::var("CARGO_PKG_VERSION").unwrap())).unwrap();
    assert!(
        Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(&bundle)
            .status()
            .expect("codesign is required")
            .success(),
        "Failed to sign macOS notification helper"
    );
}
