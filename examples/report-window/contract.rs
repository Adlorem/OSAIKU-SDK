use osaiku_ui::{
    catalog::{self, ActionKind, DataType},
    report::{Action, Contract, Field, Shape, Source},
};
pub const CONTRACT: Contract<'static> = Contract {
    catalog_version: catalog::VERSION,
    wire_version: osaiku_ui::VERSION,
    sources: &[Source {
        name: "sample",
        schema_version: 1,
        shape: Shape::Record,
    }],
    fields: &[Field {
        name: "sample-value",
        source: "sample",
        data_type: DataType::Text,
    }],
    actions: &[
        Action {
            name: "refresh-report",
            kind: ActionKind::Refresh,
        },
        Action {
            name: "change-host",
            kind: ActionKind::Navigate,
        },
    ],
};
