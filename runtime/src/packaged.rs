//! Bounded process-image scratch for packaged UI templates. Declare this as a
//! static: two Documents must not be placed on the native 16 KiB thread stack.
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};
use osaiku_ui::{Bindings, Document, Error};

struct Scratch {
    template: Document,
    bound: Document,
}
pub struct PackagedView {
    busy: AtomicBool,
    scratch: UnsafeCell<Scratch>,
}
// Every scratch access takes the guard. References never escape encode().
unsafe impl Sync for PackagedView {}

impl PackagedView {
    pub const fn new() -> Self {
        Self {
            busy: AtomicBool::new(false),
            scratch: UnsafeCell::new(Scratch {
                template: Document::EMPTY,
                bound: Document::EMPTY,
            }),
        }
    }

    /// Decode the packaged template, bind literal data and encode a replacement.
    /// On failure do not submit the output. No pointers or closures enter bytes.
    pub fn encode(
        &self,
        template: &[u8],
        data: &impl Bindings,
        focus: u32,
        output: &mut [u8],
    ) -> Result<usize, Error> {
        if self.busy.swap(true, Ordering::Acquire) {
            return Err(Error::Limit);
        }
        struct Guard<'a>(&'a AtomicBool);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _guard = Guard(&self.busy);
        // SAFETY: unique guarded access; initialized Documents live in this cell.
        let scratch = unsafe { &mut *self.scratch.get() };
        Document::decode_into(template, &mut scratch.template)?;
        scratch.template.bind_into(data, &mut scratch.bound)?;
        scratch.bound.set_focus(focus);
        scratch.bound.encode(output)
    }
}
impl Default for PackagedView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use osaiku_ui::{compiler, state, symbol, Value, MAX_WIRE};
    struct Model(bool);
    impl Bindings for Model {
        fn get(&self, name: u32) -> Option<Value<'_>> {
            if name == symbol("disabled") {
                Some(Value::Bool(self.0))
            } else if name == symbol("label") {
                Some(Value::Text("<button>Zażółć</button>"))
            } else {
                None
            }
        }
    }
    #[test]
    fn failed_preparation_recovers_and_disabled_focus_is_not_published() {
        static VIEW: PackagedView = PackagedView::new();
        let document = compiler::compile(r#"<main id="root"><button id="go" action="go" bind:text="label" bind:disabled="disabled">Go</button></main>"#, "").unwrap();
        let mut template = [0; MAX_WIRE];
        let len = document.encode(&mut template).unwrap();
        let mut output = [0; MAX_WIRE];
        assert!(VIEW
            .encode(b"invalid", &Model(false), 0, &mut output)
            .is_err());
        assert!(VIEW
            .encode(&template[..len], &Model(false), 0, &mut output[..1])
            .is_err());
        for disabled in [true, false] {
            let len = VIEW
                .encode(
                    &template[..len],
                    &Model(disabled),
                    symbol("go"),
                    &mut output,
                )
                .unwrap();
            let decoded = Document::decode(&output[..len]).unwrap();
            assert_eq!(decoded.len, 2); // Binding stayed literal, including apparent markup.
            assert_eq!(decoded.text_of(1), "<button>Zażółć</button>");
            assert_eq!(decoded.nodes[1].state & state::FOCUSED != 0, !disabled);
            assert_eq!(decoded.nodes[1].state & state::DISABLED != 0, disabled);
        }
    }

    #[test]
    fn focus_respects_hidden_and_disabled_ancestors_and_recovers() {
        static VIEW: PackagedView = PackagedView::new();
        let mut document = compiler::compile(
            r#"<main id="root"><section id="parent"><button id="go" action="go">Go</button></section></main>"#,
            "",
        )
        .unwrap();
        let mut template = [0; MAX_WIRE];
        let mut output = [0; MAX_WIRE];
        for (node, flags) in [
            (0, state::DISABLED),
            (1, state::HIDDEN),
            (2, state::HIDDEN),
            (1, state::DISABLED | state::HIDDEN),
            (0, 0),
        ] {
            for n in &mut document.nodes[..document.len as usize] {
                n.state = 0;
            }
            document.nodes[node].state = flags;
            let len = document.encode(&mut template).unwrap();
            let len = VIEW
                .encode(&template[..len], &Model(false), symbol("go"), &mut output)
                .unwrap();
            let bound = Document::decode(&output[..len]).unwrap();
            let focused = bound.nodes[..bound.len as usize]
                .iter()
                .filter(|n| n.state & state::FOCUSED != 0)
                .count();
            assert_eq!(focused, usize::from(flags == 0));
            assert_eq!(bound.nodes[2].state & state::FOCUSED != 0, flags == 0);
        }
    }
}
