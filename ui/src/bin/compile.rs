use osaiku_ui::{
    catalog,
    compiler::{compile, compile_report},
    report::parse_manifest,
    MAX_WIRE,
};
use std::io::Read;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 2 && args[0] == "--catalog" {
        let mut reference = String::new();
        catalog::write_reference(&mut reference).map_err(|e| e.to_string())?;
        return std::fs::write(&args[1], reference).map_err(|e| e.to_string());
    }
    if args.len() == 3 && args[0] == "--describe" {
        let manifest = read_source(&args[1], 32768)?;
        let manifest = parse_manifest(&manifest).map_err(|e| format!("{}: {e}", args[1]))?;
        let mut reference = String::new();
        manifest
            .contract()
            .write_reference(&mut reference)
            .map_err(|e| format!("{e:?}"))?;
        return std::fs::write(&args[2], reference).map_err(|e| e.to_string());
    }
    let contract_path = if args.len() == 5 && args[0] == "--contract" {
        Some(args[1].as_str())
    } else {
        None
    };
    let (manifest, args) = if contract_path.is_some() {
        (Some(read_source(&args[1], 32768)?), &args[2..])
    } else if args.len() == 3 {
        (None, &args[..])
    } else {
        return Err("usage: osaiku-ui-compile [--contract view.ui.contract] view.ui.html view.ui.css output.ui.bin\n       osaiku-ui-compile --catalog output.md\n       osaiku-ui-compile --describe view.ui.contract output.md".into());
    };
    let markup = read_source(&args[0], 32768)?;
    let styles = read_source(&args[1], 16384)?;
    let parsed = manifest
        .as_deref()
        .map(parse_manifest)
        .transpose()
        .map_err(|e| format!("{}: {e}", contract_path.unwrap_or("contract")))?;
    let doc = match parsed {
        Some(manifest) => compile_report(&markup, &styles, &manifest.contract()),
        None => compile(&markup, &styles),
    }
    .map_err(|e| {
        format!(
            "{}: {e}",
            if e.source == "markup" {
                &args[0]
            } else if e.source == "styles" {
                &args[1]
            } else if e.source == "contract" {
                contract_path.unwrap_or("contract")
            } else {
                e.source
            }
        )
    })?;
    let mut bytes = [0; MAX_WIRE];
    let len = doc
        .encode(&mut bytes)
        .map_err(|e| format!("encode: {e:?}"))?;
    // All validation precedes touching the destination file.
    std::fs::write(&args[2], &bytes[..len]).map_err(|e| e.to_string())?;
    println!("{} nodes, {} bytes -> {}", doc.len, len, args[2]);
    Ok(())
}

fn read_source(path: &str, limit: u64) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut source = String::new();
    file.take(limit + 1)
        .read_to_string(&mut source)
        .map_err(|e| format!("{path}: {e}"))?;
    if source.len() as u64 > limit {
        return Err(format!("{path}: source limit exceeded"));
    }
    Ok(source)
}
