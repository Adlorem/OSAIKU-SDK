//! Compose a report into an empty host-owned region using ordinary Document data.
use crate::*;

/// Both inputs remain immutable. Output is caller-owned scratch, unspecified on
/// refusal. The anchor must be an empty container; its bounds clip the report.
/// IDs stay stable and collisions with the surrounding view are refused.
pub fn mount(
    host: &Document,
    anchor: u32,
    report: &Document,
    output: &mut Document,
) -> Result<(), Error> {
    host.validate()?;
    report.validate()?;
    let anchor = host.nodes[..host.len as usize]
        .iter()
        .position(|n| n.id == anchor)
        .ok_or(Error::Structure)?;
    if host.nodes[anchor].kind != Kind::Container
        || host.nodes[..host.len as usize]
            .iter()
            .any(|n| n.parent == anchor as u16)
        || virtual_list::in_template(host, anchor)
    {
        return Err(Error::Structure);
    }
    copy(host, output);
    output.nodes[anchor].style.overflow = 1;
    output.nodes[anchor].narrow.overflow = 1;
    // Intern clipped state variants; surrounding nodes may share the originals.
    for slot in 0..14 {
        let style = output.nodes[anchor].state_styles[slot];
        if style == NO_STATE_STYLE {
            continue;
        }
        let mut clipped = output.state_styles[style as usize];
        clipped.overflow = 1;
        let index = if let Some(index) = output.state_styles[..output.state_style_len as usize]
            .iter()
            .position(|s| *s == clipped)
        {
            index
        } else {
            let index = output.state_style_len as usize;
            if index == MAX_STATE_STYLES {
                return Err(Error::Limit);
            }
            output.state_styles[index] = clipped;
            output.state_style_len += 1;
            index
        };
        output.nodes[anchor].state_styles[slot] = index as u8;
    }
    append(report, output, anchor as u16)?;
    output.validate()
}

pub(super) fn clear(document: &mut Document) {
    document.nodes.fill(Node::EMPTY);
    document.len = 0;
    document.text.fill(0);
    document.text_len = 0;
    document.points.fill(Point::EMPTY);
    document.point_len = 0;
    document.state_styles.fill(Style::DEFAULT);
    document.state_style_len = 0;
    document.decorations.fill(Decoration::NONE);
    document.decoration_len = 0;
    document.lists.fill(virtual_list::Config::EMPTY);
    document.list_len = 0;
}
pub(super) fn copy(from: &Document, to: &mut Document) {
    // No Document-sized intermediate on the native stack.
    to.nodes.copy_from_slice(&from.nodes);
    to.len = from.len;
    to.text.copy_from_slice(&from.text);
    to.text_len = from.text_len;
    to.points.copy_from_slice(&from.points);
    to.point_len = from.point_len;
    to.state_styles.copy_from_slice(&from.state_styles);
    to.state_style_len = from.state_style_len;
    to.decorations.copy_from_slice(&from.decorations);
    to.decoration_len = from.decoration_len;
    to.lists.copy_from_slice(&from.lists);
    to.list_len = from.list_len;
}

/// Extract one committed subtree, rebuilding parent/text/series offsets. This
/// removes the surrounding host's text and nodes from a saved region snapshot.
pub fn extract(from: &Document, root: u32, to: &mut Document) -> Result<(), Error> {
    from.validate()?;
    let root = from.nodes[..from.len as usize]
        .iter()
        .position(|n| n.id == root)
        .ok_or(Error::Structure)?;
    copy(from, to);
    to.nodes.fill(Node::EMPTY);
    to.len = 0;
    to.text.fill(0);
    to.text_len = 0;
    to.points.fill(Point::EMPTY);
    to.point_len = 0;
    let mut map = [NONE; MAX_NODES];
    for i in root..from.len as usize {
        let old = from.nodes[i];
        if i != root && (old.parent == NONE || map[old.parent as usize] == NONE) {
            continue;
        }
        let mut n = old;
        n.parent = if i == root {
            NONE
        } else {
            map[old.parent as usize]
        };
        map[i] = to.len;
        n.text_start = to.text_len;
        let text = from.text_of(i).as_bytes();
        let end = to.text_len as usize + text.len();
        if end > MAX_TEXT {
            return Err(Error::Limit);
        }
        to.text[to.text_len as usize..end].copy_from_slice(text);
        to.text_len = end as u16;
        n.series_start = to.point_len;
        let points =
            &from.points[old.series_start as usize..(old.series_start + old.series_len) as usize];
        let end = to.point_len as usize + points.len();
        if end > MAX_POINTS {
            return Err(Error::Limit);
        }
        to.points[to.point_len as usize..end].copy_from_slice(points);
        to.point_len = end as u16;
        to.nodes[to.len as usize] = n;
        to.len += 1;
    }
    to.validate()
}
fn append(from: &Document, to: &mut Document, parent: u16) -> Result<(), Error> {
    let nodes = to.len;
    let styles = to.state_style_len;
    let decorations = to.decoration_len;
    let lists = to.list_len;
    if usize::from(nodes + from.len) > MAX_NODES
        || usize::from(styles + from.state_style_len) > MAX_STATE_STYLES
        || usize::from(decorations + from.decoration_len) > MAX_DECORATIONS
        || usize::from(lists + from.list_len) > virtual_list::MAX_LISTS
    {
        return Err(Error::Limit);
    }
    for n in &from.nodes[..from.len as usize] {
        if to.nodes[..nodes as usize].iter().any(|old| old.id == n.id) {
            return Err(Error::Structure);
        }
    }
    let style = |mut s: Style| {
        if s.decoration != NO_DECORATION {
            s.decoration += decorations as u8;
        }
        s
    };
    for s in &from.state_styles[..from.state_style_len as usize] {
        to.state_styles[to.state_style_len as usize] = style(*s);
        to.state_style_len += 1;
    }
    for d in &from.decorations[..from.decoration_len as usize] {
        to.decorations[to.decoration_len as usize] = *d;
        to.decoration_len += 1;
    }
    for c in &from.lists[..from.list_len as usize] {
        to.lists[to.list_len as usize] = *c;
        to.list_len += 1;
    }
    for (i, n) in from.nodes[..from.len as usize].iter().enumerate() {
        let text = from.text_of(i).as_bytes();
        let points =
            &from.points[n.series_start as usize..(n.series_start + n.series_len) as usize];
        if to.text_len as usize + text.len() > MAX_TEXT
            || to.point_len as usize + points.len() > MAX_POINTS
        {
            return Err(Error::Limit);
        }
        let mut next = *n;
        next.parent = if n.parent == NONE {
            parent
        } else {
            nodes + n.parent
        };
        next.text_start = to.text_len;
        to.text[to.text_len as usize..to.text_len as usize + text.len()].copy_from_slice(text);
        to.text_len += text.len() as u16;
        next.series_start = to.point_len;
        to.points[to.point_len as usize..to.point_len as usize + points.len()]
            .copy_from_slice(points);
        to.point_len += points.len() as u16;
        next.style = style(next.style);
        next.narrow = style(next.narrow);
        for s in &mut next.state_styles {
            if *s != NO_STATE_STYLE {
                *s += styles as u8;
            }
        }
        if next.kind == Kind::VirtualList {
            next.value += lists;
        }
        to.nodes[to.len as usize] = next;
        to.len += 1;
    }
    Ok(())
}
