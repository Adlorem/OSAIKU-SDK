//! Bounded tree view with stable keys, expansion, selection and virtual rows.
//! The host retains the model; visible rows carry no authority over their data.
use crate::{layout::Rect, virtual_list, Error, MAX_DEPTH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Node {
    key: u64,
    parent: u16,
    depth: u8,
    branch: bool,
}
const EMPTY: Node = Node {
    key: 0,
    parent: u16::MAX,
    depth: 0,
    branch: false,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub key: u64,
    pub index: usize,
    pub depth: u8,
    pub branch: bool,
    pub expanded: bool,
    pub selected: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Navigation {
    Previous,
    Next,
    Parent,
    Child,
    First,
    Last,
}

pub struct Tree<const N: usize> {
    nodes: [Node; N],
    len: usize,
    expanded: [u64; N],
    selected: u64,
}
impl<const N: usize> Tree<N> {
    pub const NEW: Self = Self {
        nodes: [EMPTY; N],
        len: 0,
        expanded: [0; N],
        selected: 0,
    };
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn selected(&self) -> u64 {
        self.selected
    }
    pub fn index(&self, key: u64) -> Option<usize> {
        self.nodes[..self.len].iter().position(|n| n.key == key)
    }
    /// Parents must already exist. This makes cycles impossible, without recursion.
    pub fn push(&mut self, key: u64, parent: u64, branch: bool) -> Result<usize, Error> {
        if key == 0 || self.index(key).is_some() {
            return Err(Error::Value);
        }
        if self.len >= N || self.len >= u16::MAX as usize {
            return Err(Error::Limit);
        }
        let (parent, depth) = if parent == 0 {
            (u16::MAX, 0)
        } else {
            let p = self.index(parent).ok_or(Error::Structure)?;
            if !self.nodes[p].branch {
                return Err(Error::Structure);
            }
            (p as u16, self.nodes[p].depth as usize + 1)
        };
        if depth >= MAX_DEPTH {
            return Err(Error::Limit);
        }
        let i = self.len;
        self.nodes[i] = Node {
            key,
            parent,
            depth: depth as u8,
            branch,
        };
        self.len += 1;
        Ok(i)
    }
    pub fn expanded(&self, key: u64) -> bool {
        key != 0 && self.expanded.contains(&key)
    }
    pub fn set_expanded(&mut self, key: u64, expand: bool) -> Result<(), Error> {
        let i = self.index(key).ok_or(Error::Value)?;
        if !self.nodes[i].branch {
            return Err(Error::Structure);
        }
        if expand {
            if !self.expanded(key) {
                *self
                    .expanded
                    .iter_mut()
                    .find(|v| **v == 0)
                    .ok_or(Error::Limit)? = key;
            }
        } else {
            for v in &mut self.expanded {
                if *v == key {
                    *v = 0;
                }
            }
            // Collapsing the ancestor of the selection moves selection to it.
            if self
                .index(self.selected)
                .is_some_and(|s| self.descendant(s, i))
            {
                self.selected = key;
            }
        }
        Ok(())
    }
    fn descendant(&self, mut i: usize, parent: usize) -> bool {
        while self.nodes[i].parent != u16::MAX {
            i = self.nodes[i].parent as usize;
            if i == parent {
                return true;
            }
        }
        false
    }
    pub fn select(&mut self, key: u64) -> Result<(), Error> {
        if !self.visible().any(|r| r.key == key) {
            return Err(Error::Value);
        }
        self.selected = key;
        Ok(())
    }
    /// Rebuild the observations while retaining UI state by key, never by row number.
    pub fn begin_refresh(&mut self) {
        self.len = 0;
    }
    pub fn finish_refresh(&mut self) {
        for i in 0..N {
            if self
                .index(self.expanded[i])
                .is_none_or(|n| !self.nodes[n].branch)
            {
                self.expanded[i] = 0;
            }
        }
        if !self.visible().any(|r| r.key == self.selected) {
            self.selected = self.visible().next().map_or(0, |r| r.key);
        }
    }
    pub fn visible(&self) -> Visible<'_, N> {
        Visible {
            tree: self,
            next: (self.len != 0).then_some(0),
            all: false,
        }
    }
    /// Every row in the same order, as if each branch were expanded: a
    /// filtered view (a search) shows matches inside collapsed branches.
    /// `expanded` still reports the retained state.
    pub fn every(&self) -> Visible<'_, N> {
        Visible {
            tree: self,
            next: (self.len != 0).then_some(0),
            all: true,
        }
    }
    pub fn navigate(&mut self, direction: Navigation) -> Option<u64> {
        let current = self.index(self.selected);
        let next = match direction {
            Navigation::First => self.visible().next().map(|r| r.key),
            Navigation::Last => self.visible().last().map(|r| r.key),
            Navigation::Previous => self
                .visible()
                .take_while(|r| r.key != self.selected)
                .last()
                .map(|r| r.key),
            Navigation::Next => self
                .visible()
                .skip_while(|r| r.key != self.selected)
                .nth(1)
                .map(|r| r.key),
            Navigation::Parent => {
                let i = current?;
                if self.expanded(self.selected) {
                    self.set_expanded(self.selected, false).ok()?;
                    Some(self.selected)
                } else {
                    self.nodes.get(self.nodes[i].parent as usize).map(|n| n.key)
                }
            }
            Navigation::Child => {
                let i = current?;
                if self.nodes[i].branch && !self.expanded(self.selected) {
                    self.set_expanded(self.selected, true).ok()?;
                    Some(self.selected)
                } else {
                    self.nodes[..self.len]
                        .iter()
                        .find(|n| n.parent as usize == i)
                        .map(|n| n.key)
                }
            }
        };
        if let Some(key) = next {
            self.select(key).ok()?;
        }
        Some(self.selected)
    }
}
#[derive(Clone)]
pub struct Visible<'a, const N: usize> {
    tree: &'a Tree<N>,
    next: Option<usize>,
    all: bool,
}
impl<const N: usize> Iterator for Visible<'_, N> {
    type Item = Row;
    fn next(&mut self) -> Option<Row> {
        let index = self.next?;
        let n = self.tree.nodes[index];
        let expanded = self.tree.expanded(n.key);
        self.next = if n.branch && (expanded || self.all) {
            self.tree.nodes[..self.tree.len]
                .iter()
                .position(|v| v.parent as usize == index)
        } else {
            None
        };
        if self.next.is_none() {
            let mut current = index;
            loop {
                let parent = self.tree.nodes[current].parent;
                self.next =
                    (current + 1..self.tree.len).find(|i| self.tree.nodes[*i].parent == parent);
                if self.next.is_some() || parent == u16::MAX {
                    break;
                }
                current = parent as usize;
            }
        }
        Some(Row {
            key: n.key,
            index,
            depth: n.depth,
            branch: n.branch,
            expanded,
            selected: n.key == self.tree.selected,
        })
    }
}

#[derive(Clone, Copy)]
pub struct Geometry {
    pub row_height: u16,
    pub indent: u16,
    pub toggle_width: u16,
}
impl Geometry {
    pub fn range(
        self,
        total: usize,
        scroll: u32,
        viewport: Rect,
        capacity: usize,
    ) -> Result<virtual_list::Range, Error> {
        virtual_list::range(
            virtual_list::Config {
                source: 1,
                item_height: self.row_height,
                overscan: 1,
                total: u32::try_from(total).map_err(|_| Error::Limit)?,
                first: 0,
            },
            scroll,
            viewport.height,
            capacity,
        )
    }
    /// One shared geometry for paint, disclosure hit targets and selection.
    pub fn row(
        self,
        viewport: Rect,
        ordinal: u32,
        depth: u8,
        scroll: u32,
    ) -> Result<(Rect, Rect), Error> {
        if self.row_height == 0 || depth as usize >= MAX_DEPTH {
            return Err(Error::Value);
        }
        let y = i64::from(viewport.y) + i64::from(ordinal) * i64::from(self.row_height)
            - i64::from(scroll);
        let left = u32::from(depth) * u32::from(self.indent);
        let inset = left.min(viewport.width);
        let toggle = Rect {
            x: viewport.x + inset as i32,
            y: i32::try_from(y).map_err(|_| Error::Limit)?,
            width: u32::from(self.toggle_width).min(viewport.width - inset),
            height: u32::from(self.row_height),
        };
        let label = Rect {
            x: toggle.x + toggle.width as i32,
            width: viewport.width - inset - toggle.width,
            ..toggle
        };
        Ok((toggle, label))
    }
}
