//! Shared by every delivered UI application. Host tools remain ordinary Rust.
pub fn configure() {
    println!("cargo:rustc-check-cfg=cfg(osaiku_sdk_gpu)");
    println!("cargo:rerun-if-env-changed=OSAIKU_SDK_CLOSURE");
    println!("cargo:rerun-if-env-changed=OSAIKU_COMPOSITOR_CLOSURE");
    println!("cargo:rerun-if-env-changed=OSAIKU_SDK_CPU_ONLY");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none") {
        return;
    }
    assert!(closure().is_some() || std::env::var("OSAIKU_SDK_CPU_ONLY").as_deref()==Ok("1"),
        "UI release requires OSAIKU_SDK_CLOSURE (or OSAIKU_COMPOSITOR_CLOSURE); diagnostic CPU builds require OSAIKU_SDK_CPU_ONLY=1");
    if let Some(object) = closure() {
        assert!(
            std::path::Path::new(&object).is_file(),
            "missing SDK Mesa closure"
        );
        println!("cargo:rustc-cfg=osaiku_sdk_gpu");
        println!("cargo:rerun-if-changed={object}");
        println!("cargo:rustc-link-arg={object}");
        println!("cargo:rustc-link-arg=--eh-frame-hdr");
    }
}
fn closure() -> Option<String> {
    std::env::var("OSAIKU_SDK_CLOSURE")
        .or_else(|_| std::env::var("OSAIKU_COMPOSITOR_CLOSURE"))
        .ok()
}
pub fn linker(original: &std::path::Path) -> std::path::PathBuf {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none") && closure().is_some() {
        std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("../../sdk/runtime/gpu-linker.ld")
    } else {
        original.to_path_buf()
    }
}
