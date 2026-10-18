//! Process-owned report workspace. Its guarded storage avoids large native
//! stack temporaries; admission/publication and permissions remain host policy.
pub mod artifact;
pub mod metadata;
pub mod transfer;
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};
pub use osaiku_ui::report::host::{Prepared, Session, Surface};
use osaiku_ui::{
    report::{
        host::{Error, Host},
        Contract,
    },
    Bindings, Document,
};

struct Scratch {
    host: Host,
    template: Document,
    bound: Document,
    definitions: transfer::Inbox,
    fresh: bool,
}
pub struct ReportView {
    busy: AtomicBool,
    scratch: UnsafeCell<Scratch>,
}
// Each access holds busy. No reference to scratch escapes a public method.
unsafe impl Sync for ReportView {}
impl ReportView {
    pub const fn new() -> Self {
        Self {
            busy: AtomicBool::new(false),
            scratch: UnsafeCell::new(Scratch {
                host: Host::EMPTY,
                template: Document::EMPTY,
                bound: Document::EMPTY,
                definitions: transfer::Inbox::EMPTY,
                fresh: false,
            }),
        }
    }
    fn with<T>(&self, f: impl FnOnce(&mut Scratch) -> Result<T, Error>) -> Result<T, Error> {
        if self.busy.swap(true, Ordering::Acquire) {
            return Err(Error::Busy);
        }
        struct Guard<'a>(&'a AtomicBool);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _guard = Guard(&self.busy);
        // SAFETY: the guard grants exclusive access; references cannot escape.
        f(unsafe { &mut *self.scratch.get() })
    }
    pub fn open(&self, owner: u64, report: u64, surface: Surface) -> Result<Session, Error> {
        self.with(|s| s.host.open(owner, report, surface))
    }
    pub fn revision(&self, session: Session) -> Result<u64, Error> {
        self.with(|s| s.host.revision(session))
    }
    /// Accept only packets from the trusted lifecycle host and for this report's
    /// latest accepted publication. `sender` comes from kernel IPC, never bytes.
    /// Finish validates the complete wire against host-owned source/action
    /// registration; it stages a definition but does not claim publication.
    pub fn receive_definition(
        &self,
        session: Session,
        host: crate::application::ApplicationHost,
        sender: crate::process::ProcessEndpoint,
        contract: &Contract<'_>,
        bytes: &[u8],
    ) -> Result<Option<transfer::Reply>, Error> {
        self.receive_catalog_definition(session, host, sender, contract, bytes, &[])
    }
    /// Like receive_definition, with application-owned immutable catalog entries.
    pub fn receive_catalog_definition(
        &self,
        session: Session,
        host: crate::application::ApplicationHost,
        sender: crate::process::ProcessEndpoint,
        contract: &Contract<'_>,
        bytes: &[u8],
        catalog: &[&[u8]],
    ) -> Result<Option<transfer::Reply>, Error> {
        host.classify(sender, bytes).map_err(|_| Error::Owner)?;
        if !transfer::candidate(bytes) {
            return Ok(None);
        }
        let packet = transfer::Packet::decode(bytes).ok_or(Error::View(osaiku_ui::Error::Value))?;
        self.with(|s| {
            let current = s.host.accepted(session)?.ok_or(Error::Stale)?;
            let revision = s.host.revision(session)?;
            if s.host.check_update(session, revision) == Err(Error::Busy) {
                return Ok(Some(transfer::Reply::for_packet(
                    packet,
                    transfer::Status::Busy,
                    0,
                )));
            }
            Ok(Some(s.definitions.receive_catalog(
                packet,
                current,
                catalog,
                |wire| {
                    Document::decode_into(wire, &mut s.template).is_ok()
                        && contract.validate_document(&s.template).is_ok()
                },
            )))
        })
    }
    /// Stage a registered template and literal bindings, optionally inside a
    /// trusted surrounding view. Unknown references are refused before callbacks.
    pub fn prepare(
        &self,
        session: Session,
        base: u64,
        contract: &Contract<'_>,
        template: &[u8],
        bindings: &impl Bindings,
        surrounding: Option<&[u8]>,
        output: &mut [u8],
    ) -> Result<(Prepared, usize), Error> {
        self.with(|s| {
            // Reject stale/foreign callers before resolving any provider.
            s.host.check_update(session, base)?;
            let result = (|| {
                let template = s.definitions.definition(template);
                Document::decode_into(template, &mut s.template)?;
                contract
                    .validate_document(&s.template)
                    .map_err(Error::Contract)?;
                s.template.bind_into(bindings, &mut s.bound)?;
                // Binding consumed the template. Reuse its phase buffer for
                // the surrounding view instead of a third full Document.
                let surrounding = if let Some(wire) = surrounding {
                    Document::decode_into(wire, &mut s.template)?;
                    Some(&s.template)
                } else {
                    None
                };
                let prepared = if s.fresh {
                    s.host
                        .prepare_fresh(session, base, contract, &s.bound, surrounding)?
                } else {
                    s.host
                        .prepare(session, base, contract, &s.bound, surrounding)?
                };
                match s.host.encode(prepared, output) {
                    Ok(len) => Ok((prepared, len)),
                    Err(error) => {
                        s.host.discard(prepared)?;
                        Err(error)
                    }
                }
            })();
            if result.is_err() && s.definitions.staged() {
                s.definitions.discard();
                s.fresh = false;
            }
            result
        })
    }
    pub fn encode(&self, prepared: Prepared, output: &mut [u8]) -> Result<usize, Error> {
        self.with(|s| s.host.encode(prepared, output))
    }
    /// Save the accepted definition or the actually committed report subtree.
    /// Only explicitly named persistence controls remain live in a snapshot.
    pub fn save_artifact(
        &self,
        session: Session,
        fallback: &[u8],
        mut header: artifact::Header,
        live_fields: &[u32],
        live_actions: &[u32],
        output: &mut [u8],
    ) -> Result<usize, Error> {
        self.with(|s| {
            s.host.check_update(session, s.host.revision(session)?)?;
            if s.definitions.busy() {
                return Err(Error::Busy);
            }
            header.recorded = s.host.accepted(session)?.ok_or(Error::Stale)?;
            let payload = output.get_mut(artifact::HEADER..).ok_or(Error::Limit)?;
            let n = if header.kind == artifact::DEFINITION {
                let wire = s.definitions.definition(fallback);
                payload
                    .get_mut(..wire.len())
                    .ok_or(Error::Limit)?
                    .copy_from_slice(wire);
                wire.len()
            } else if header.kind == artifact::SNAPSHOT {
                let current = s.host.current(session)?.ok_or(Error::Stale)?;
                let root = match session.surface {
                    Surface::Window => current.nodes[0].id,
                    Surface::Region(anchor) => {
                        let index = current.nodes[..current.len as usize]
                            .iter()
                            .position(|n| n.id == anchor)
                            .ok_or(Error::Stale)?;
                        current.nodes[..current.len as usize]
                            .iter()
                            .find(|n| n.parent == index as u16)
                            .ok_or(Error::Stale)?
                            .id
                    }
                };
                osaiku_ui::report::region::extract(current, root, &mut s.bound)?;
                for node in &mut s.bound.nodes[..s.bound.len as usize] {
                    // Virtual providers have no stable materialized rows in v1.
                    if node.kind == osaiku_ui::Kind::VirtualList {
                        return Err(Error::View(osaiku_ui::Error::Version));
                    }
                    for binding in &mut node.bindings {
                        if !live_fields.contains(binding) {
                            *binding = 0;
                        }
                    }
                    if node.kind == osaiku_ui::Kind::TextInput
                        || (node.action != 0 && !live_actions.contains(&node.action))
                    {
                        node.state |= osaiku_ui::state::DISABLED;
                    }
                }
                s.bound.encode(payload)?
            } else {
                return Err(Error::View(osaiku_ui::Error::Version));
            };
            header.seal(output, n).map_err(|_| Error::Limit)
        })
    }
    pub fn open_artifact(
        &self,
        session: Session,
        contract: &Contract<'_>,
        recipe: u64,
        live_fields: &[u32],
        live_actions: &[u32],
        bytes: &[u8],
    ) -> Result<artifact::Header, Error> {
        self.with(|s| {
            s.host.check_update(session, s.host.revision(session)?)?;
            if s.definitions.busy() {
                return Err(Error::Busy);
            }
            let (header, wire) = artifact::Header::parse(bytes)
                .map_err(|_| Error::View(osaiku_ui::Error::Version))?;
            header
                .compatible(contract, recipe)
                .map_err(|_| Error::View(osaiku_ui::Error::Version))?;
            Document::decode_into(wire, &mut s.template)?;
            contract
                .validate_document(&s.template)
                .map_err(Error::Contract)?;
            if header.kind == artifact::SNAPSHOT
                && s.template.nodes[..s.template.len as usize].iter().any(|n| {
                    n.kind == osaiku_ui::Kind::VirtualList
                        || n.bindings
                            .iter()
                            .any(|b| *b != 0 && !live_fields.contains(b))
                        || ((n.kind == osaiku_ui::Kind::TextInput
                            || (n.action != 0 && !live_actions.contains(&n.action)))
                            && n.state & osaiku_ui::state::DISABLED == 0)
                })
            {
                return Err(Error::View(osaiku_ui::Error::Binding));
            }
            let id = s.host.accepted(session)?.ok_or(Error::Stale)?;
            if !s.definitions.stage_local(wire, id) {
                return Err(Error::Busy);
            }
            s.fresh = true;
            Ok(header)
        })
    }
    pub fn reset_definition(&self, session: Session) -> Result<(), Error> {
        self.with(|s| {
            s.host.check_update(session, s.host.revision(session)?)?;
            s.definitions.close();
            s.fresh = true;
            Ok(())
        })
    }
    pub fn commit(&self, prepared: Prepared, identity: osaiku_ui::Identity) -> Result<(), Error> {
        self.with(|s| {
            s.host.commit(prepared, identity)?;
            s.definitions.commit();
            s.fresh = false;
            Ok(())
        })
    }
    pub fn discard(&self, prepared: Prepared) -> Result<(), Error> {
        self.with(|s| {
            s.host.discard(prepared)?;
            s.fresh = false;
            if s.definitions.staged() {
                s.definitions.discard();
            }
            Ok(())
        })
    }
    pub fn event(
        &self,
        session: Session,
        event: osaiku_ui::input::Event,
    ) -> Result<Option<osaiku_ui::catalog::ActionKind>, Error> {
        self.with(|s| s.host.event(session, event))
    }
    pub fn set_filter(
        &self,
        session: Session,
        identity: osaiku_ui::Identity,
        element: u32,
        text: &str,
    ) -> Result<(), Error> {
        self.with(|s| s.host.set_filter(session, identity, element, text))
    }
    pub fn close(&self, session: Session) -> Result<(), Error> {
        self.with(|s| {
            s.host.close(session)?;
            s.definitions.close();
            s.fresh = false;
            // In-place reset, including unused tails. Documents never move onto
            // the stack, and the next session cannot observe old source values.
            for document in [&mut s.template, &mut s.bound] {
                document.nodes.fill(osaiku_ui::Node::EMPTY);
                document.len = 0;
                document.text.fill(0);
                document.text_len = 0;
                document.points.fill(osaiku_ui::Point::EMPTY);
                document.point_len = 0;
                document.state_styles.fill(osaiku_ui::Style::DEFAULT);
                document.state_style_len = 0;
                document.decorations.fill(osaiku_ui::Decoration::NONE);
                document.decoration_len = 0;
                document.lists.fill(osaiku_ui::virtual_list::Config::EMPTY);
                document.list_len = 0;
            }
            Ok(())
        })
    }
}
impl Default for ReportView {
    fn default() -> Self {
        Self::new()
    }
}
