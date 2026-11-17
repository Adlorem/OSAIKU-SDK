//! Delivered UI applications link the public CPU scene client only. Mesa is
//! selected at runtime from the signed GPU package, never from a build path.
pub fn configure() {
    println!("cargo:rustc-check-cfg=cfg(osaiku_sdk_gpu)");
    println!("cargo:rerun-if-env-changed=OSAIKU_SDK_CPU_ONLY");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none")
        && std::env::var("OSAIKU_SDK_CPU_ONLY").as_deref() != Ok("1")
    {
        println!("cargo:rustc-cfg=osaiku_sdk_gpu");
    }
}
pub fn linker(original: &std::path::Path) -> std::path::PathBuf {
    original.to_path_buf()
}
