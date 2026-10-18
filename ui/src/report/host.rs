//! One bounded report session with transactional view replacement. Store this
//! object in caller-owned heap/image storage, not on a native thread stack.
//! Owner IDs identify local sessions; callers authenticate external requests.
use super::{catalog::ActionKind, region, Contract, Shape};
use crate::{input, state, symbol, Document, Identity, Kind, MAX_NODES, MAX_TEXT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Window,
    Region(u32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    pub owner: u64,
    pub report: u64,
    pub generation: u64,
    pub surface: Surface,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prepared {
    session: Session,
    serial: u64,
    revision: u64,
}
impl Prepared {
    pub fn revision(self) -> u64 {
        self.revision
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Contract(super::Error),
    View(crate::Error),
    Owner,
    Stale,
    Busy,
    Closed,
    Limit,
}
impl From<crate::Error> for Error {
    fn from(error: crate::Error) -> Self {
        Self::View(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SourceKey {
    id: u32,
    version: u16,
    shape: Shape,
    data_type: super::DataType,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Compatible {
    kind: Kind,
    parent: u32,
    action: u32,
    intention: Option<ActionKind>,
    bindings: [u32; 6],
    sources: [Option<SourceKey>; 6],
    row_source: u32,
    row_version: u16,
}
#[derive(Clone, Copy)]
struct Interaction {
    compatible: Option<Compatible>,
    selected: Option<bool>,
    scroll: Option<u32>,
    focus: Option<bool>,
    filter_set: bool,
    filter_len: u16,
    filter: [u8; crate::MAX_LEAF_TEXT],
}
impl Interaction {
    const EMPTY: Self = Self {
        compatible: None,
        selected: None,
        scroll: None,
        focus: None,
        filter_set: false,
        filter_len: 0,
        filter: [0; crate::MAX_LEAF_TEXT],
    };
}
struct Slot {
    document: Document,
    interaction: [Interaction; MAX_NODES],
}
impl Slot {
    const EMPTY: Self = Self {
        document: Document::EMPTY,
        interaction: [Interaction::EMPTY; MAX_NODES],
    };
    fn clear(&mut self) {
        region::clear(&mut self.document);
        self.interaction.fill(Interaction::EMPTY);
    }
}
pub struct Host {
    session: Option<Session>,
    serial: u64,
    revision: u64,
    current: usize,
    pending: Option<Prepared>,
    accepted: Option<Identity>,
    slots: [Slot; 2],
    text: [u8; MAX_TEXT],
}
impl Host {
    pub const EMPTY: Self = Self {
        session: None,
        serial: 0,
        revision: 0,
        current: 0,
        pending: None,
        accepted: None,
        slots: [const { Slot::EMPTY }; 2],
        text: [0; MAX_TEXT],
    };

    /// Open a fresh incarnation. A region is owned by the surrounding window's
    /// process and names an empty container in that process's trusted view.
    pub fn open(&mut self, owner: u64, report: u64, surface: Surface) -> Result<Session, Error> {
        if self.session.is_some() {
            return Err(Error::Busy);
        }
        if owner == 0 || report == 0 || surface == Surface::Region(0) {
            return Err(Error::Owner);
        }
        let serial = self.serial.checked_add(1).ok_or(Error::Limit)?;
        let session = Session {
            owner,
            report,
            surface,
            generation: serial,
        };
        self.serial = serial;
        self.session = Some(session);
        Ok(session)
    }
    fn check(&self, session: Session) -> Result<(), Error> {
        match self.session {
            None => Err(Error::Closed),
            Some(current) if current != session => Err(Error::Owner),
            _ => Ok(()),
        }
    }
    pub fn revision(&self, session: Session) -> Result<u64, Error> {
        self.check(session)?;
        Ok(self.revision)
    }
    pub fn accepted(&self, session: Session) -> Result<Option<Identity>, Error> {
        self.check(session)?;
        Ok(self.accepted)
    }
    pub fn check_update(&self, session: Session, base_revision: u64) -> Result<(), Error> {
        self.check(session)?;
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if base_revision != self.revision {
            return Err(Error::Stale);
        }
        Ok(())
    }
    pub fn current(&self, session: Session) -> Result<Option<&Document>, Error> {
        self.check(session)?;
        Ok((self.revision != 0).then_some(&self.slots[self.current].document))
    }

    /// Stage already-bound report data. Registration is host-owned. Region
    /// composition uses an immutable surrounding view and an empty anchor.
    /// Source/provider execution happens before this call, outside the host.
    pub fn prepare(
        &mut self,
        session: Session,
        base_revision: u64,
        contract: &Contract<'_>,
        report: &Document,
        surrounding: Option<&Document>,
    ) -> Result<Prepared, Error> {
        self.prepare_mode(session, base_revision, contract, report, surrounding, true)
    }
    /// Opening a stored artifact starts with its recorded state, not interaction
    /// state retained from an unrelated report with coincidentally equal IDs.
    pub fn prepare_fresh(
        &mut self,
        session: Session,
        base_revision: u64,
        contract: &Contract<'_>,
        report: &Document,
        surrounding: Option<&Document>,
    ) -> Result<Prepared, Error> {
        self.prepare_mode(session, base_revision, contract, report, surrounding, false)
    }
    fn prepare_mode(
        &mut self,
        session: Session,
        base_revision: u64,
        contract: &Contract<'_>,
        report: &Document,
        surrounding: Option<&Document>,
        preserve: bool,
    ) -> Result<Prepared, Error> {
        self.check_update(session, base_revision)?;
        let serial = self.serial.checked_add(1).ok_or(Error::Limit)?;
        let revision = self.revision.checked_add(1).ok_or(Error::Limit)?;
        contract
            .validate_document(report)
            .map_err(Error::Contract)?;
        let index = 1 - self.current;
        let result = self.stage(index, contract, report, surrounding, preserve);
        self.text.fill(0);
        if let Err(error) = result {
            self.slots[index].clear();
            return Err(error);
        }
        let token = Prepared {
            session,
            serial,
            revision,
        };
        self.serial = serial;
        self.pending = Some(token);
        Ok(token)
    }
    fn stage(
        &mut self,
        index: usize,
        contract: &Contract<'_>,
        report: &Document,
        surrounding: Option<&Document>,
        preserve: bool,
    ) -> Result<(), Error> {
        let (old, next) = if index == 1 {
            let (a, b) = self.slots.split_at_mut(1);
            (&a[0], &mut b[0])
        } else {
            let (a, b) = self.slots.split_at_mut(1);
            (&b[0], &mut a[0])
        };
        next.clear();
        match (self.session.ok_or(Error::Closed)?.surface, surrounding) {
            (Surface::Window, None) => region::copy(report, &mut next.document),
            (Surface::Region(anchor), Some(host)) => {
                region::mount(host, anchor, report, &mut next.document)?
            }
            _ => return Err(Error::Owner),
        }
        for (i, node) in report.nodes[..report.len as usize].iter().enumerate() {
            let target = next.document.nodes[..next.document.len as usize]
                .iter()
                .position(|n| n.id == node.id)
                .ok_or(Error::Owner)?;
            let compatible = compatibility(contract, report, i);
            let previous = old.document.nodes[..old.document.len as usize]
                .iter()
                .position(|n| n.id == node.id);
            next.interaction[target] = previous
                .filter(|j| {
                    preserve
                        && (compatible.row_source == 0 || node.kind == Kind::VirtualList)
                        && old.interaction[*j].compatible == Some(compatible)
                })
                .map(|j| old.interaction[j])
                .unwrap_or(Interaction::EMPTY);
            next.interaction[target].compatible = Some(compatible);
        }
        // Repack text, including edited filters, into process-owned scratch.
        // This also handles valid input documents with overlapping text ranges.
        let mut len = 0;
        for i in 0..next.document.len as usize {
            let interaction = &next.interaction[i];
            let bytes = if interaction.filter_set {
                &interaction.filter[..interaction.filter_len as usize]
            } else {
                next.document.text_of(i).as_bytes()
            };
            let end = len + bytes.len();
            self.text
                .get_mut(len..end)
                .ok_or(Error::Limit)?
                .copy_from_slice(bytes);
            let text_len = bytes.len() as u16;
            let n = &mut next.document.nodes[i];
            n.text_start = len as u16;
            n.text_len = text_len;
            len = end;
            if let Some(selected) = interaction.selected {
                n.state = (n.state & !state::SELECTED) | if selected { state::SELECTED } else { 0 };
            }
            if let Some(scroll) = interaction.scroll {
                n.scroll_y = if n.style.overflow == 2 || n.narrow.overflow == 2 {
                    scroll
                } else {
                    0
                };
            }
            if let Some(focus) = interaction.focus {
                n.state = (n.state & !state::FOCUSED) | if focus { state::FOCUSED } else { 0 };
            }
        }
        next.document.text.copy_from_slice(&self.text);
        next.document.text_len = len as u16;
        // Authoritative restrictions can invalidate retained focus.
        let focus = next.interaction[..next.document.len as usize]
            .iter()
            .enumerate()
            .find(|(_, interaction)| interaction.focus == Some(true))
            .map(|(i, _)| next.document.nodes[i].id)
            .or_else(|| {
                next.document.nodes[..next.document.len as usize]
                    .iter()
                    .find(|n| n.state & state::FOCUSED != 0)
                    .map(|n| n.id)
            })
            .unwrap_or(0);
        next.document.set_focus(focus);
        next.document.validate()?;
        Ok(())
    }
    fn token(&self, token: Prepared) -> Result<(), Error> {
        self.check(token.session)?;
        if self.pending != Some(token) {
            return Err(Error::Stale);
        }
        Ok(())
    }
    pub fn prepared(&self, token: Prepared) -> Result<&Document, Error> {
        self.token(token)?;
        Ok(&self.slots[1 - self.current].document)
    }
    pub fn encode(&self, token: Prepared, output: &mut [u8]) -> Result<usize, Error> {
        Ok(self.prepared(token)?.encode(output)?)
    }
    /// Commit only after the ordinary surface/window submission succeeds. This
    /// records acceptance, not a claim that the display has scanned out pixels.
    pub fn commit(&mut self, token: Prepared, identity: Identity) -> Result<(), Error> {
        self.token(token)?;
        if identity.owner != token.session.owner
            || identity.window == 0
            || identity.incarnation == 0
        {
            return Err(Error::Owner);
        }
        if identity.epoch == 0 || identity.revision == 0 {
            return Err(Error::Stale);
        }
        if let Some(old) = self.accepted {
            if identity.window != old.window || identity.incarnation != old.incarnation {
                return Err(Error::Owner);
            }
            if identity.epoch < old.epoch
                || (identity.epoch == old.epoch && identity.revision <= old.revision)
            {
                return Err(Error::Stale);
            }
        }
        self.current = 1 - self.current;
        self.revision = token.revision;
        self.accepted = Some(identity);
        self.pending = None;
        Ok(())
    }
    pub fn discard(&mut self, token: Prepared) -> Result<(), Error> {
        self.token(token)?;
        self.slots[1 - self.current].clear();
        self.pending = None;
        Ok(())
    }
    fn input_index(
        &self,
        session: Session,
        identity: Identity,
        element: u32,
    ) -> Result<usize, Error> {
        self.check(session)?;
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.accepted != Some(identity) {
            return Err(Error::Stale);
        }
        let slot = &self.slots[self.current];
        let index = slot.document.nodes[..slot.document.len as usize]
            .iter()
            .position(|n| n.id == element)
            .ok_or(Error::Stale)?;
        if slot.interaction[index].compatible.is_none() {
            return Err(Error::Owner);
        }
        let mut ancestor = index;
        loop {
            let n = &slot.document.nodes[ancestor];
            if n.kind == Kind::Template || n.state & (state::HIDDEN | state::DISABLED) != 0 {
                return Err(crate::Error::Disabled.into());
            }
            if n.parent == crate::NONE {
                break;
            }
            ancestor = n.parent as usize;
        }
        Ok(index)
    }
    /// Input must originate from the authenticated ordinary host input route.
    /// Surrounding-region controls and stale revisions cannot alter the report.
    pub fn event(
        &mut self,
        session: Session,
        event: input::Event,
    ) -> Result<Option<ActionKind>, Error> {
        let index = self.input_index(session, event.target, event.element)?;
        let slot = &mut self.slots[self.current];
        event.validate(event.target, &slot.document)?;
        match event.kind {
            input::FOCUS => {
                for interaction in &mut slot.interaction {
                    if interaction.compatible.is_some() {
                        interaction.focus = Some(false);
                    }
                }
                slot.interaction[index].focus = Some(true);
                Ok(None)
            }
            input::SCROLL => {
                slot.interaction[index].scroll = Some(event.value as u32);
                Ok(None)
            }
            input::ACTIVATE => {
                let compatible = slot.interaction[index].compatible.ok_or(Error::Owner)?;
                if compatible.intention == Some(ActionKind::Select) {
                    // Virtualized ordinals are not stable data keys. Selection
                    // reconciliation for those rows belongs to the data provider.
                    if compatible.row_source == 0 {
                        slot.interaction[index].selected = Some(true);
                    }
                }
                Ok(compatible.intention)
            }
            _ => Err(crate::Error::Value.into()),
        }
    }
    pub fn set_filter(
        &mut self,
        session: Session,
        identity: Identity,
        element: u32,
        text: &str,
    ) -> Result<(), Error> {
        let index = self.input_index(session, identity, element)?;
        let slot = &mut self.slots[self.current];
        let compatible = slot.interaction[index].compatible.ok_or(Error::Owner)?;
        if compatible.kind != Kind::TextInput || compatible.intention != Some(ActionKind::Filter) {
            return Err(Error::Owner);
        }
        if text.len() > crate::MAX_LEAF_TEXT {
            return Err(Error::Limit);
        }
        let interaction = &mut slot.interaction[index];
        interaction.filter.fill(0);
        interaction.filter[..text.len()].copy_from_slice(text.as_bytes());
        interaction.filter_len = text.len() as u16;
        interaction.filter_set = true;
        Ok(())
    }
    pub fn filter(&self, session: Session, element: u32) -> Result<&str, Error> {
        self.check(session)?;
        let slot = &self.slots[self.current];
        let index = slot.document.nodes[..slot.document.len as usize]
            .iter()
            .position(|n| n.id == element)
            .ok_or(Error::Stale)?;
        let interaction = &slot.interaction[index];
        if !interaction
            .compatible
            .is_some_and(|c| c.kind == Kind::TextInput && c.intention == Some(ActionKind::Filter))
        {
            return Err(Error::Owner);
        }
        if interaction.filter_set {
            core::str::from_utf8(&interaction.filter[..interaction.filter_len as usize])
                .map_err(|_| Error::View(crate::Error::Text))
        } else {
            Ok(slot.document.text_of(index))
        }
    }
    /// Closing revokes both prepared and accepted revisions and scrubs all
    /// report/filter bytes. A later open receives a new incarnation.
    pub fn close(&mut self, session: Session) -> Result<(), Error> {
        self.check(session)?;
        for slot in &mut self.slots {
            slot.clear();
        }
        self.text.fill(0);
        self.session = None;
        self.pending = None;
        self.accepted = None;
        self.revision = 0;
        self.current = 0;
        Ok(())
    }
}
fn compatibility(contract: &Contract<'_>, document: &Document, index: usize) -> Compatible {
    let n = &document.nodes[index];
    let mut row_source = 0;
    let mut ancestor = index;
    loop {
        let node = &document.nodes[ancestor];
        if node.kind == Kind::VirtualList {
            row_source = document.lists[node.value as usize].source;
            break;
        }
        if node.parent == crate::NONE {
            break;
        }
        ancestor = node.parent as usize;
    }
    let source_for = |field: &&super::Field<'_>| {
        contract.sources.iter().find(|s| {
            s.name == field.source
                && if row_source == 0 || n.kind == Kind::VirtualList {
                    s.shape == Shape::Record
                } else {
                    s.shape == Shape::Rows && symbol(s.name) == row_source
                }
        })
    };
    let mut sources = [None; 6];
    for (slot, binding) in n.bindings.iter().enumerate() {
        if *binding == 0 {
            continue;
        }
        if let Some((field, source)) = contract
            .fields
            .iter()
            .filter(|f| symbol(f.name) == *binding)
            .find_map(|field| source_for(&field).map(|source| (field, source)))
        {
            sources[slot] = Some(SourceKey {
                id: symbol(source.name),
                version: source.schema_version,
                shape: source.shape,
                data_type: field.data_type,
            });
        }
    }
    Compatible {
        kind: n.kind,
        parent: if n.parent == crate::NONE {
            0
        } else {
            document.nodes[n.parent as usize].id
        },
        action: n.action,
        intention: contract
            .actions
            .iter()
            .find(|a| symbol(a.name) == n.action)
            .map(|a| a.kind),
        bindings: n.bindings,
        sources,
        row_source,
        row_version: contract
            .sources
            .iter()
            .find(|s| symbol(s.name) == row_source)
            .map_or(0, |s| s.schema_version),
    }
}
