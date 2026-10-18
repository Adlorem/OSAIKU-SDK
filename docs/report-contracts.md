# Report compilation and catalogs

The experimental catalog v1 is available without an allocator through
`osaiku_ui::catalog`. The compiler and wire validator use these same component,
binding and action-target descriptors. The [generated catalog](ui-catalog.md)
lists components, semantic roles, binding types, local report intentions and
limits. It is independent of the document wire major, which remains 10.

Reports use the ordinary native compiler, shared control styles, `Document`
and wire encoding. `compiler::compile_report` adds admission against a
host-registered `report::Contract`; it does not introduce a markup dialect.
The compiler CLI is the first dynamic consumer. A packaged app can use the
same entry point from its build script.

## Host registration

Hosts define sources with a name, schema version and `Record` or `Rows` shape.
Fields declare a source, name and `Text`, `Percent`, `Series` or `Bool` type.
Actions map names to application-local intentions. The schemas describe
read-only binding values; application-owned state may also be a record source.
Action handlers remain ordinary host code, outside the document.

`Contract::validate_document` checks a compiled or decoded document before the
host resolves providers. Unknown references, wrong types, wrong source scopes,
ambiguous symbols, malformed structure and unsupported versions are refused.
Only fields belonging to a virtual list's registered row source can appear in
its prototype or materialized rows. Row sources may reuse field names; record fields must have
unambiguous names across all record sources. Scalar record bindings are outside
prototypes. These rules match `Document::bind_into` and `virtual_list::Items`.

The contract must come from the host's registrations. Accepting a contract
supplied by untrusted report text is not source authorization. Neither a schema
version nor a symbol hash authenticates data or grants access. Real providers
must enforce their own scoped query permissions and disclosure policy. Validate
received wire again against the host contract, even if a producer says it used
`compile_report`. The wire itself remains an ordinary document; it carries no
new capability or claim that a manifest was checked.

Use the normal input validation against the exact presented identity/revision
before resolving an action with `Contract::action`. A declared `refresh` or
`close` is only an intention: this catalog does not execute it. A report cannot
register a disk write, firmware action or arbitrary command.

## Manifest and CLI

A `.ui.contract` file is a bounded textual representation for build tools and
user-space hosts. It is at most 32 KiB, with at most 16 sources, 128 fields and
64 actions. Names use the same 1–64 ASCII name rules as markup. Blank lines and
whole-line `#` comments are allowed. Versions must be positive integers; catalog
and wire versions require an exact match. Source versions describe schemas
registered by the host, not automatically negotiated service versions.

```text
report 1 10
source summary record 1
source entries rows 1
field title summary text
field name entries text
field selected entries bool
action reload refresh
action choose select
```

Declarations may refer to sources appearing later in the same file. Unknown
directives, extra tokens, duplicate declarations and unresolved sources fail.
`parse_manifest` borrows names from the source string and owns only bounded
descriptor vectors. Runtime `Contract` validation needs no allocator.

```console
osaiku-ui-compile --catalog ui-catalog.md
osaiku-ui-compile --describe report.ui.contract report-api.md
osaiku-ui-compile --contract report.ui.contract report.ui.html report.ui.css report.ui.bin
```

All validation and encoding complete before the CLI opens the output file.
A refused contract or view preserves the previous artifact. The legacy
three-argument compilation command is unchanged. Rust hosts can construct a
`Contract` directly or use `parse_manifest`, then pass `&manifest.contract()`
to `compile_report`. `Contract::write_reference` generates the source/field/action
API reference from those same registrations.

This is the 30F1 foundation. The [transactional report host](report-host.md)
implements the reusable SDK lifecycle and region composition, including OS report
and assistant surfaces. The [query provider](report-queries.md) implements bounded
reads, sorting/filtering and category data. [Window context](window-context.md)
provides coherent image/semantic capture with separate disclosure authority.
Saved report artifacts remain subsequent 30F work. Declaring local intentions
does not establish query or disclosure authority.
