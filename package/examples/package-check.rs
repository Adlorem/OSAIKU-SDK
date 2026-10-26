//! Host inspection/verification adapter. Signing and private keys are absent.
use osaiku_package::{
    application::Package,
    policy::PolicyDocument,
    repository::{Index, RepositoryState},
    system::Release,
    *,
};
use std::{env, fs::File, io::Read};
fn read(path: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("file exceeds limit".into());
    }
    Ok(bytes)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 || !value.is_ascii() {
        return Err("expected 32-byte hex digest".into());
    }
    let mut out = [0; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(|_| "bad hex")?;
    }
    Ok(out)
}
fn run() -> Result<(), String> {
    let a: Vec<_> = env::args().skip(1).collect();
    let arg = |i: usize| {
        a.get(i)
            .map(String::as_str)
            .ok_or_else(|| "missing argument".to_string())
    };
    let command = arg(0)?;
    let bytes = read(
        arg(1)?,
        if command.starts_with("source-") {
            repository::sources::BYTES as u64
        } else if command.ends_with("index") {
            MAX_INDEX_BYTES
        } else {
            MAX_PACKAGE_BYTES
        },
    )?;
    let err = |e: Error| format!("{e:?}");
    match command {
        "inspect" | "verify" | "inspect-driver" | "verify-driver" => {
            let package = if command.ends_with("-driver") {
                driver::parse(&bytes)
            } else {
                Package::parse(&bytes)
            }
            .map_err(err)?;
            println!(
                "id={}\nname={:?}\nversion={}\nrelease={}\npublisher={}\nbytes={}\nsha256={}",
                package.manifest().id(),
                package.manifest().name(),
                package.manifest().version(),
                package.manifest().release(),
                hex(&package.publisher()),
                bytes.len(),
                hex(&digest(&bytes))
            );
            if command.ends_with("-driver") {
                println!(
                    "classes={}\nresources={}",
                    driver::classes(package.manifest()).map_err(err)?,
                    package.manifest().permissions()
                );
            } else {
                println!("category={}", package.manifest().category_name());
            }
            if command == "verify" || command == "verify-driver" {
                let policy_bytes = read(arg(2)?, policy::MAX_BYTES as u64)?;
                let policy = PolicyDocument::parse(&policy_bytes).map_err(err)?;
                let verified = package.verify(&policy.policy()).map_err(err)?;
                let floor = arg(3)?.parse().map_err(|_| "bad release floor")?;
                let rollback = if arg(4)? == "-" {
                    None
                } else {
                    Some(unhex(arg(4)?)?)
                };
                verified.check_rollback(floor, rollback).map_err(err)?;
                println!("verified=true");
            } else {
                println!("verified=false");
            }
        }
        "source-application" | "source-driver" => {
            let domain = if command == "source-driver" {
                Domain::Driver
            } else {
                Domain::Application
            };
            let source = repository::sources::Source::parse(&bytes, domain).map_err(err)?;
            println!(
                "source=true\ndomain={domain:?}\nidentity={}\npublisher_trust=false",
                hex(&source.identity().map_err(err)?)
            );
        }
        "inspect-index" | "verify-index" | "inspect-driver-index" | "verify-driver-index" => {
            let driver = command.ends_with("driver-index");
            let index = Index::parse_for(
                &bytes,
                if driver {
                    Domain::Driver
                } else {
                    Domain::Application
                },
            )
            .map_err(err)?;
            println!(
                "repository={}\nsequence={}\nentries={}",
                index.id(),
                index.sequence(),
                index.entries().len()
            );
            if command.starts_with("verify-") {
                let policy_bytes = read(arg(2)?, policy::MAX_BYTES as u64)?;
                let policy = PolicyDocument::parse(&policy_bytes).map_err(err)?;
                let now = if arg(3)? == "-" {
                    None
                } else {
                    Some(arg(3)?.parse().map_err(|_| "bad time")?)
                };
                let state = RepositoryState {
                    id: arg(4)?,
                    sequence: arg(5)?.parse().map_err(|_| "bad sequence")?,
                    digest: unhex(arg(6)?)?,
                };
                let checkpoint = index.verify(&policy.policy(), now, &state).map_err(err)?;
                // Optional exact package binding for a selected entry.
                if let Some(path) = a.get(7) {
                    let app_bytes = read(path, MAX_PACKAGE_BYTES)?;
                    let package = (if driver {
                        driver::parse(&app_bytes)
                    } else {
                        Package::parse(&app_bytes)
                    })
                    .map_err(err)?
                    .verify(&policy.policy())
                    .map_err(err)?;
                    let entry = index
                        .entries()
                        .find(|e| {
                            e.id() == package.manifest().id()
                                && e.publisher() == package.publisher()
                        })
                        .ok_or("entry absent")?;
                    entry.bind(&package).map_err(err)?;
                }
                println!("verified=true\ncheckpoint={}", hex(&checkpoint));
            } else {
                println!("verified=false");
            }
        }
        "verify-system" => {
            let policy_bytes = read(arg(2)?, policy::MAX_BYTES as u64)?;
            let policy = PolicyDocument::parse(&policy_bytes).map_err(err)?;
            Release::parse(&bytes)
                .map_err(err)?
                .verify(&policy.policy())
                .map_err(err)?;
            println!("verified=true");
        }
        "policy" => {
            PolicyDocument::parse(&bytes).map_err(err)?;
            println!("policy=true");
        }
        _ => return Err("unknown command".into()),
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("refused: {error}");
        std::process::exit(1);
    }
}
