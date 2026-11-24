fn main() {
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
