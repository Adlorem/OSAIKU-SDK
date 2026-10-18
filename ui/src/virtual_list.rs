//! Fixed-height virtual lists. Only the viewport and bounded overscan are bound.
//! Providers run in the application; no callback or data-source authority is encoded.
use crate::{layout::Layout, *};

pub const MAX_LISTS: usize = 16;
pub const MAX_EXTENT: u32 = 1_048_576;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub source: u32,
    pub item_height: u16,
    pub overscan: u16,
    pub total: u32,
    pub first: u32,
}
impl Config {
    pub const EMPTY: Self = Self {
        source: 0,
        item_height: 0,
        overscan: 0,
        total: 0,
        first: 0,
    };
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub first: u32,
    pub count: u32,
    pub scroll: u32,
}
/// IDs are stable for an ordinal in a list. Consumers must still resolve input
/// against its presented revision and the provider's stable item identity.
pub fn item_id(list: u32, template_node: u32, ordinal: u32) -> u32 {
    let mut hash = 2166136261u32;
    for byte in list
        .to_le_bytes()
        .into_iter()
        .chain(template_node.to_le_bytes())
        .chain(ordinal.to_le_bytes())
    {
        hash = (hash ^ u32::from(byte)).wrapping_mul(16777619);
    }
    hash.max(1)
}
pub fn range(config: Config, scroll: u32, height: u32, capacity: usize) -> Result<Range, Error> {
    if config.item_height == 0 || config.item_height > 4096 || config.overscan > 8 {
        return Err(Error::Value);
    }
    let stride = u32::from(config.item_height);
    let extent = config
        .total
        .checked_mul(stride)
        .filter(|v| *v <= MAX_EXTENT)
        .ok_or(Error::Limit)?;
    let scroll = scroll.min(extent.saturating_sub(height));
    if height == 0 || config.total == 0 {
        return Ok(Range {
            scroll,
            ..Range::default()
        });
    }
    let first_visible = scroll / stride;
    let end = scroll
        .saturating_add(height)
        .div_ceil(stride)
        .min(config.total);
    let visible = (end - first_visible) as usize;
    if visible > capacity {
        return Err(Error::Limit);
    }
    let before = u32::from(config.overscan)
        .min(first_visible)
        .min((capacity - visible) as u32);
    let after = u32::from(config.overscan)
        .min(config.total - end)
        .min((capacity - visible) as u32 - before);
    Ok(Range {
        first: first_visible - before,
        count: end - first_visible + before + after,
        scroll,
    })
}
pub fn in_template(view: &Document, mut index: usize) -> bool {
    for _ in 0..MAX_DEPTH {
        let Some(n) = view
            .nodes
            .get(index)
            .filter(|_| index < usize::from(view.len))
        else {
            return false;
        };
        if n.kind == Kind::Template {
            return true;
        }
        if n.parent == NONE {
            return false;
        }
        index = usize::from(n.parent);
    }
    false
}
pub(crate) fn validate(view: &Document) -> Result<(), Error> {
    let mut next = 0;
    for (i, n) in view.nodes[..view.len as usize].iter().enumerate() {
        if n.kind == Kind::Template
            && (n.parent == NONE || view.nodes[n.parent as usize].kind != Kind::VirtualList)
        {
            return Err(Error::Structure);
        }
        if n.kind != Kind::VirtualList {
            if n.bindings[2] != 0 && in_template(view, i) {
                return Err(Error::Binding);
            }
            continue;
        }
        if n.value != next || next >= view.list_len {
            return Err(Error::Structure);
        }
        next += 1;
        let c = view.lists[n.value as usize];
        if c.source == 0
            || c.item_height == 0
            || c.item_height > 4096
            || c.overscan > 8
            || c.first > c.total
            || c.total
                .checked_mul(u32::from(c.item_height))
                .is_none_or(|v| v > MAX_EXTENT)
            || n.style.overflow != 2
            || n.narrow.overflow != 2
            || in_template(view, i)
        {
            return Err(Error::Value);
        }
        let mut children = view.nodes[..view.len as usize]
            .iter()
            .enumerate()
            .filter(|(_, child)| child.parent == i as u16);
        let first = children.next();
        if let Some((prototype, _)) = first.filter(|(_, child)| child.kind == Kind::Template) {
            if children.next().is_some()
                || c.first != 0
                || view.nodes[..view.len as usize]
                    .iter()
                    .filter(|n| n.parent == prototype as u16)
                    .count()
                    != 1
            {
                return Err(Error::Structure);
            }
        } else {
            let count = usize::from(first.is_some()) + children.count();
            if count > (c.total - c.first) as usize
                || view.nodes[..view.len as usize]
                    .iter()
                    .any(|child| child.parent == i as u16 && child.kind == Kind::Template)
            {
                return Err(Error::Structure);
            }
        }
    }
    if next != view.list_len {
        return Err(Error::Structure);
    }
    Ok(())
}
/// Application data can come from a page cache, filesystem cursor or memory.
/// Missing bindings refuse the replacement; callers retain the previous frame.
pub trait Items {
    fn get(&self, source: u32, ordinal: u32, binding: u32) -> Option<Value<'_>>;
}
/// Use the very same layout as painting/input. Hidden lists request no data.
/// All list totals/scroll positions must be set before querying any range:
/// visible lists share one node budget, with visible rows ahead of overscan.
pub fn requested(view: &Document, layout: &Layout, index: usize) -> Result<Range, Error> {
    let n = view
        .nodes
        .get(index)
        .filter(|n| index < view.len as usize && n.kind == Kind::VirtualList)
        .ok_or(Error::Structure)?;
    let ranges = plan(view, layout)?;
    Ok(ranges[n.value as usize])
}

/// Reserve all visible rows before distributing spare nodes as overscan. Every
/// query and realization uses this same plan, so two lists cannot each spend
/// the whole document budget. Parent-before-child order lets us count template
/// descendants in one pass without repeatedly walking their ancestor chains.
fn plan(view: &Document, layout: &Layout) -> Result<[Range; MAX_LISTS], Error> {
    view.validate()?;
    if layout.len != view.len as usize {
        return Err(Error::Structure);
    }
    let mut ranges = [Range::default(); MAX_LISTS];
    let mut owners = [NONE; MAX_NODES];
    let mut costs = [0usize; MAX_LISTS];
    let mut indices = [0usize; MAX_LISTS];
    let mut heights = [0u32; MAX_LISTS];
    let mut base = view.len as usize;
    for (i, n) in view.nodes[..view.len as usize].iter().enumerate() {
        owners[i] = if n.kind == Kind::Template {
            view.nodes[n.parent as usize].value
        } else if n.parent != NONE {
            owners[n.parent as usize]
        } else {
            NONE
        };
        if owners[i] != NONE {
            base -= 1;
            if n.kind != Kind::Template {
                costs[owners[i] as usize] += 1;
            }
        }
        if n.kind == Kind::VirtualList {
            indices[n.value as usize] = i;
        }
    }
    let mut remaining = MAX_NODES - base;
    for slot in 0..view.list_len as usize {
        if costs[slot] == 0 {
            return Err(Error::Structure);
        }
        let index = indices[slot];
        let placed = layout.nodes[index];
        if placed.hidden || placed.clip.width == 0 || placed.clip.height == 0 {
            continue;
        }
        heights[slot] = placed
            .bounds
            .inset(view.insets(layout.style(view, index)))
            .height;
        ranges[slot] = range(
            Config {
                overscan: 0,
                ..view.lists[slot]
            },
            view.nodes[index].scroll_y,
            heights[slot],
            remaining / costs[slot],
        )?;
        remaining -= ranges[slot].count as usize * costs[slot];
    }
    for slot in 0..view.list_len as usize {
        let visible = ranges[slot].count as usize;
        if visible == 0 {
            continue;
        }
        ranges[slot] = range(
            view.lists[slot],
            ranges[slot].scroll,
            heights[slot],
            visible + remaining / costs[slot],
        )?;
        remaining -= (ranges[slot].count as usize - visible) * costs[slot];
    }
    Ok(ranges)
}
/// Expand a prepared template into caller-owned scratch, with no heap and no
/// Document-sized stack temporary. Never publish `out` after an error.
pub fn realize(
    view: &Document,
    layout: &Layout,
    items: &impl Items,
    out: &mut Document,
) -> Result<(), Error> {
    let ranges = plan(view, layout)?;
    out.nodes.fill(Node::EMPTY);
    out.len = 0;
    out.text.fill(0);
    out.text_len = 0;
    out.points.copy_from_slice(&view.points);
    out.point_len = view.point_len;
    out.state_styles.copy_from_slice(&view.state_styles);
    out.state_style_len = view.state_style_len;
    out.decorations.copy_from_slice(&view.decorations);
    out.decoration_len = view.decoration_len;
    out.lists.fill(Config::EMPTY);
    out.list_len = 0;
    copy(view, &ranges, items, out, 0, NONE, None)?;
    out.validate()
}
fn copy(
    view: &Document,
    ranges: &[Range; MAX_LISTS],
    items: &impl Items,
    out: &mut Document,
    index: usize,
    parent: u16,
    item: Option<(u32, u32, u32)>,
) -> Result<(), Error> {
    if out.len as usize == MAX_NODES {
        return Err(Error::Limit);
    }
    let mut n = view.nodes[index];
    n.parent = parent;
    if n.kind == Kind::VirtualList {
        // Traversal order can differ from serialized parent-before-child order.
        // The emitted table must follow the emitted virtual-list nodes.
        let slot = n.value as usize;
        n.value = out.list_len;
        out.lists[out.list_len as usize] = Config {
            first: ranges[slot].first,
            ..view.lists[slot]
        };
        out.list_len += 1;
        n.scroll_y = ranges[slot].scroll;
    }
    let text = if let Some((source, list, ordinal)) = item {
        n.id = item_id(list, n.id, ordinal);
        if n.bindings[2] != 0 {
            return Err(Error::Binding);
        }
        if n.bindings[1] != 0 {
            n.value = match items.get(source, ordinal, n.bindings[1]) {
                Some(Value::Percent(v)) => v.unwrap_or(10001),
                _ => return Err(Error::Binding),
            };
        }
        for (slot, flag) in [
            (3, state::HIDDEN),
            (4, state::DISABLED),
            (5, state::SELECTED),
        ] {
            if n.bindings[slot] != 0 {
                match items.get(source, ordinal, n.bindings[slot]) {
                    Some(Value::Bool(v)) => n.state = (n.state & !flag) | if v { flag } else { 0 },
                    _ => return Err(Error::Binding),
                }
            }
        }
        if n.bindings[0] == 0 {
            view.text_of(index)
        } else {
            match items.get(source, ordinal, n.bindings[0]) {
                Some(Value::Text(v)) => v,
                _ => return Err(Error::Binding),
            }
        }
    } else {
        view.text_of(index)
    };
    if text.len() > MAX_LEAF_TEXT || out.text_len as usize + text.len() > MAX_TEXT {
        return Err(Error::Limit);
    }
    n.text_start = out.text_len;
    n.text_len = text.len() as u16;
    out.text[out.text_len as usize..out.text_len as usize + text.len()]
        .copy_from_slice(text.as_bytes());
    out.text_len += text.len() as u16;
    let at = out.len;
    out.nodes[at as usize] = n;
    out.len += 1;
    if n.kind == Kind::VirtualList {
        let slot = view.nodes[index].value as usize;
        let r = ranges[slot];
        let template = view.nodes[..view.len as usize]
            .iter()
            .position(|child| child.parent == index as u16 && child.kind == Kind::Template)
            .ok_or(Error::Structure)?;
        let root = view.nodes[..view.len as usize]
            .iter()
            .position(|child| child.parent == template as u16)
            .ok_or(Error::Structure)?;
        for ordinal in r.first..r.first + r.count {
            copy(
                view,
                ranges,
                items,
                out,
                root,
                at,
                Some((view.lists[slot].source, n.id, ordinal)),
            )?;
        }
    } else {
        for child in 0..view.len as usize {
            if view.nodes[child].parent == index as u16 {
                copy(view, ranges, items, out, child, at, item)?;
            }
        }
    }
    Ok(())
}
