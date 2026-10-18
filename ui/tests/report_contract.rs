#![cfg(feature = "compiler")]
use osaiku_ui::{
    catalog,
    compiler::{compile, compile_report},
    report::{self, parse_manifest},
    *,
};

const CONTRACT: &str = "report 1 11\nsource summary record 1\nsource entries rows 2\nsource other rows 1\nfield title summary text\nfield hidden summary bool\nfield usage summary percent\nfield history summary series\nfield name entries text\nfield name other percent\naction refresh refresh\naction choose select\n";
const MARKUP: &str = r#"<main id="root"><span id="title" bind:text="title"></span><section id="details" bind:hidden="hidden"><ui-value-bar id="meter" bind:value="usage"></ui-value-bar><ui-chart id="chart" bind:series="history"></ui-chart></section><button id="reload" action="refresh">Refresh</button><ui-virtual-list id="list" source="entries"><template><ui-list-item id="row" action="choose"><span id="name" bind:text="name"></span></ui-list-item></template></ui-virtual-list></main>"#;

#[test]
fn report_compilation_preserves_the_ordinary_native_wire() {
    let manifest = parse_manifest(CONTRACT).unwrap();
    let report = compile_report(MARKUP, "", &manifest.contract()).unwrap();
    let native = compile(MARKUP, "").unwrap();
    let mut report_bytes = [0; MAX_WIRE];
    let mut native_bytes = [0; MAX_WIRE];
    let len = report.encode(&mut report_bytes).unwrap();
    assert_eq!(native.encode(&mut native_bytes).unwrap(), len);
    assert_eq!(report_bytes, native_bytes);
    let decoded = Document::decode(&report_bytes[..len]).unwrap();
    manifest.contract().validate_document(&decoded).unwrap();
    assert_eq!(
        manifest.contract().action(symbol("refresh")),
        Ok(catalog::ActionKind::Refresh)
    );
    assert_eq!(
        manifest.contract().action(symbol("arbitrary")),
        Err(report::Error::UnknownAction)
    );
}

#[test]
fn fields_are_typed_and_scoped_before_any_provider_is_called() {
    let manifest = parse_manifest(CONTRACT).unwrap();
    let check = |markup: &str| {
        manifest
            .contract()
            .validate_document(&compile(markup, "").unwrap())
    };
    assert_eq!(
        check(r#"<span id="root" bind:text="usage"></span>"#),
        Err(report::Error::Type)
    );
    assert_eq!(
        check(r#"<span id="root" bind:text="missing"></span>"#),
        Err(report::Error::UnknownField)
    );
    assert_eq!(
        check(r#"<span id="root" bind:text="name"></span>"#),
        Err(report::Error::Scope)
    );
    assert_eq!(
        check(&MARKUP.replace("bind:text=\"name\"", "bind:text=\"title\"")),
        Err(report::Error::Scope)
    );
    // Same field spelling has independently typed meanings in different row sources.
    assert_eq!(
        check(&MARKUP.replace("source=\"entries\"", "source=\"other\"")),
        Err(report::Error::Type)
    );
    assert_eq!(
        check(&MARKUP.replace("source=\"entries\"", "source=\"summary\"")),
        Err(report::Error::Scope)
    );
    assert_eq!(
        check(&MARKUP.replace("source=\"entries\"", "source=\"absent\"")),
        Err(report::Error::UnknownSource)
    );
    assert_eq!(
        check(&MARKUP.replace("action=\"refresh\"", "action=\"shutdown\"")),
        Err(report::Error::UnknownAction)
    );
}

#[test]
fn malformed_wire_cannot_bypass_structural_admission() {
    let manifest = parse_manifest(CONTRACT).unwrap();
    let mut doc = compile(MARKUP, "").unwrap();
    doc.nodes[1].parent = 1; // Would otherwise make the scope walk loop forever.
    assert_eq!(
        manifest.contract().validate_document(&doc),
        Err(report::Error::Document(Error::Structure))
    );
    doc.len = u16::MAX;
    assert_eq!(
        manifest.contract().validate_document(&doc),
        Err(report::Error::Document(Error::Limit))
    );
}

#[test]
fn manifests_refuse_ambiguous_names_unsupported_versions_and_unbounded_inputs() {
    assert_eq!(symbol("costarring"), symbol("liquid"));
    assert!(
        parse_manifest("report 1 11\nsource costarring record 1\naction liquid refresh").is_err()
    );
    for text in [
        "",
        "report 2 11",
        "report 1 10",
        "report 1 11\nreport 1 11",
        "report 1 11 extra",
        "report 1 11\nsource a record 0",
        "report 1 11\nsource a mutable 1",
        "report 1 11\nsource a record 1\nsource a rows 1",
        "report 1 11\nfield f missing text",
        "report 1 11\nsource a record 1\nfield f a pointer",
        "report 1 11\nsource a record 1\nfield f a text\nfield f a bool",
        "report 1 11\nsource a record 1\nsource b record 1\nfield f a text\nfield f b text",
        "report 1 11\naction a refresh\naction a close",
        "report 1 11\naction a write-disk",
        "report 1 11\nsource <script> record 1",
    ] {
        assert!(parse_manifest(text).is_err(), "accepted {text}");
    }
    assert!(parse_manifest(&" ".repeat(32769)).is_err());
    let mut sources = String::from("report 1 11\n");
    for i in 0..=report::MAX_SOURCES {
        sources.push_str(&format!("source s{i} record 1\n"));
    }
    assert!(parse_manifest(&sources)
        .unwrap_err()
        .message
        .contains("limit"));
    let manifest = parse_manifest(CONTRACT).unwrap();
    let mut contract = manifest.contract();
    contract.catalog_version += 1;
    assert_eq!(
        contract.action(symbol("refresh")),
        Err(report::Error::Version)
    );
}

#[test]
fn compiler_diagnostics_identify_contract_failures() {
    let manifest = parse_manifest(CONTRACT).unwrap();
    let err = compile_report(
        r#"<span id="root" bind:text="unknown"></span>"#,
        "",
        &manifest.contract(),
    )
    .unwrap_err();
    assert_eq!(err.source, "contract");
    assert!(err.message.contains("UnknownField"));
    let err = parse_manifest("# comment\nreport 1 11\naction a exec extra").unwrap_err();
    assert_eq!((err.source, err.line), ("contract", 3));
}

#[test]
fn catalog_reference_is_generated_from_the_current_compiler_contract() {
    let mut output = String::new();
    catalog::write_reference(&mut output).unwrap();
    assert_eq!(
        output,
        include_str!("../../docs/ui-catalog.md").replace("\r\n", "\n")
    );
    for binding in catalog::BINDINGS {
        assert!(binding.slot < 6);
        assert_eq!(
            catalog::binding(binding.attribute).unwrap().data_type,
            binding.data_type
        );
    }
}

#[test]
fn cli_refusal_keeps_the_existing_artifact_and_success_uses_native_wire() {
    let path = std::env::temp_dir().join(format!("osaiku-report-contract-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let markup = path.join("view.html");
    let styles = path.join("view.css");
    let schema = path.join("view.contract");
    let output = path.join("view.bin");
    std::fs::write(&markup, MARKUP).unwrap();
    std::fs::write(&styles, "").unwrap();
    std::fs::write(
        &schema,
        CONTRACT.replace("field title summary text", "field title summary bool"),
    )
    .unwrap();
    std::fs::write(&output, b"previous artifact").unwrap();
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_osaiku-ui-compile"))
            .arg("--contract")
            .arg(&schema)
            .arg(&markup)
            .arg(&styles)
            .arg(&output)
            .output()
            .unwrap()
    };
    let refused = run();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("Type"));
    assert_eq!(std::fs::read(&output).unwrap(), b"previous artifact");
    std::fs::write(&schema, CONTRACT).unwrap();
    let accepted = run();
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let wire = std::fs::read(&output).unwrap();
    parse_manifest(CONTRACT)
        .unwrap()
        .contract()
        .validate_document(&Document::decode(&wire).unwrap())
        .unwrap();
    let reference = path.join("api.md");
    let described = std::process::Command::new(env!("CARGO_BIN_EXE_osaiku-ui-compile"))
        .arg("--describe")
        .arg(&schema)
        .arg(&reference)
        .output()
        .unwrap();
    assert!(described.status.success());
    let previous = std::fs::read(&reference).unwrap();
    assert!(String::from_utf8_lossy(&previous).contains("| `entries` | `name` | text |"));
    std::fs::write(&schema, "report 99 11").unwrap();
    let refused = std::process::Command::new(env!("CARGO_BIN_EXE_osaiku-ui-compile"))
        .arg("--describe")
        .arg(&schema)
        .arg(&reference)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert_eq!(std::fs::read(&reference).unwrap(), previous);
    std::fs::remove_dir_all(path).unwrap();
}
