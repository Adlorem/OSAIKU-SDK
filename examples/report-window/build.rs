use std::{env, path::PathBuf};
mod contract;
fn main() {
    println!("cargo:rerun-if-changed=contract.rs");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for name in ["report", "region"] {
        let html = format!("{name}.ui.html");
        let css = format!("{name}.ui.css");
        println!("cargo:rerun-if-changed={html}");
        println!("cargo:rerun-if-changed={css}");
        let markup = std::fs::read_to_string(html).unwrap();
        let styles = std::fs::read_to_string(css).unwrap();
        let document = if name == "report" {
            osaiku_ui::compiler::compile_report(&markup, &styles, &contract::CONTRACT)
        } else {
            osaiku_ui::compiler::compile(&markup, &styles)
        }
        .unwrap();
        let mut wire = [0; osaiku_ui::MAX_WIRE];
        let len = document.encode(&mut wire).unwrap();
        std::fs::write(output.join(format!("{name}.ui.bin")), &wire[..len]).unwrap();
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none") {
        let script = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("../../runtime/linker.ld");
        println!("cargo:rerun-if-changed={}", script.display());
        println!("cargo:rustc-link-arg=-T{}", script.display());
    }
}
