//! One bounded compiler for packaged views and dynamic user-space hosts.
use crate::*;
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub source: &'static str,
    pub line: usize,
    pub column: usize,
    pub message: String,
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.source, self.line, self.column, self.message
        )
    }
}
type Result<T> = core::result::Result<T, Diagnostic>;
struct Parser<'a> {
    source: &'static str,
    text: &'a str,
    pos: usize,
}
impl<'a> Parser<'a> {
    fn error(&self, message: impl Into<String>) -> Diagnostic {
        let prefix = &self.text[..self.pos];
        Diagnostic {
            source: self.source,
            line: prefix.bytes().filter(|b| *b == b'\n').count() + 1,
            column: prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
            message: message.into(),
        }
    }
    fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }
    fn ws(&mut self) {
        self.pos += self.rest().len() - self.rest().trim_start().len();
    }
    fn take(&mut self, token: &str) -> bool {
        if self.rest().starts_with(token) {
            self.pos += token.len();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, token: &str) -> Result<()> {
        if self.take(token) {
            Ok(())
        } else {
            Err(self.error(format!("expected '{token}'")))
        }
    }
    fn name(&mut self) -> Result<&'a str> {
        let start = self.pos;
        while let Some(b) = self.text.as_bytes().get(self.pos) {
            if !b.is_ascii_alphanumeric() && !b"-_:".contains(b) {
                break;
            }
            self.pos += 1;
        }
        if start == self.pos {
            return Err(self.error("expected name"));
        }
        Ok(&self.text[start..self.pos])
    }
    fn quoted(&mut self) -> Result<String> {
        let quote = if self.take("\"") {
            '"'
        } else if self.take("'") {
            '\''
        } else {
            return Err(self.error("attribute value must be quoted"));
        };
        let end = self
            .rest()
            .find(quote)
            .ok_or_else(|| self.error("unclosed attribute"))?;
        let result =
            entities(&self.rest()[..end]).ok_or_else(|| self.error("unsupported entity"))?;
        self.pos += end + 1;
        Ok(result)
    }
}
fn entities(text: &str) -> Option<String> {
    let mut result = String::new();
    let mut rest = text;
    while let Some(index) = rest.find('&') {
        result.push_str(&rest[..index]);
        rest = &rest[index..];
        let (entity, ch) = [
            ("&amp;", '&'),
            ("&lt;", '<'),
            ("&gt;", '>'),
            ("&quot;", '"'),
            ("&apos;", '\''),
        ]
        .into_iter()
        .find(|(e, _)| rest.starts_with(e))?;
        result.push(ch);
        rest = &rest[entity.len()..];
    }
    result.push_str(rest);
    Some(result)
}
fn kind(tag: &str) -> Option<Kind> {
    catalog::component(tag).map(|component| component.kind)
}
struct Meta {
    tag: String,
    id: String,
    classes: Vec<String>,
}
struct Names(Vec<String>);
impl Names {
    fn intern(&mut self, name: &str, p: &Parser<'_>) -> Result<u32> {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(
                p.error("names require 1..64 ASCII letters, digits, dash, dot or underscore")
            );
        }
        let id = symbol(name);
        if id == 0 || self.0.iter().any(|old| old != name && symbol(old) == id) {
            return Err(p.error("symbol hash collision"));
        }
        if !self.0.iter().any(|s| s == name) {
            self.0.push(name.into());
        }
        Ok(id)
    }
}

/// Compile a native application view over the shared SDK control styles.
pub fn compile(markup: &str, css: &str) -> Result<Document> {
    compile_view(markup, css, theme::CONTROLS)
}

/// Compile generated markup through the ordinary native compiler, then admit
/// its data/action references against a host-registered, versioned contract.
pub fn compile_report(
    markup: &str,
    css: &str,
    contract: &report::Contract<'_>,
) -> Result<Document> {
    let error = |e| Diagnostic {
        source: "contract",
        line: 1,
        column: 1,
        message: format!("report contract: {e:?}"),
    };
    contract.validate().map_err(error)?;
    let document = compile(markup, css)?;
    contract.validate_document(&document).map_err(error)?;
    Ok(document)
}

/// Compile an explicitly styled surface, such as host-owned window chrome or
/// a custom canvas. Standard application controls should use `compile`.
pub fn compile_unstyled(markup: &str, css: &str) -> Result<Document> {
    compile_view(markup, css, "")
}

fn compile_view(markup: &str, css: &str, defaults: &str) -> Result<Document> {
    let mut p = Parser {
        source: "markup",
        text: markup,
        pos: 0,
    };
    if markup.len() > 32768 {
        return Err(p.error("markup exceeds 32 KiB"));
    }
    let mut view = Document::EMPTY;
    let mut meta: Vec<Meta> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut names = Names(Vec::new());
    loop {
        p.ws();
        if p.rest().is_empty() {
            break;
        }
        if p.take("<!--") {
            let end = p
                .rest()
                .find("-->")
                .ok_or_else(|| p.error("unclosed comment"))?;
            p.pos += end + 3;
            continue;
        }
        if p.take("</") {
            let tag = p.name()?;
            p.ws();
            p.expect(">")?;
            let i = stack
                .pop()
                .ok_or_else(|| p.error("unexpected closing element"))?;
            if meta[i].tag != tag {
                return Err(p.error("mismatched closing element"));
            }
            continue;
        }
        p.expect("<")?;
        let tag = p.name()?;
        let kind = kind(tag).ok_or_else(|| p.error(format!("unsupported element '{tag}'")))?;
        let i = view.len as usize;
        if i == MAX_NODES || stack.len() >= MAX_DEPTH {
            return Err(p.error("node/depth limit exceeded"));
        }
        if i != 0 && stack.is_empty() {
            return Err(p.error("exactly one root element required"));
        }
        let mut n = Node {
            kind,
            parent: stack.last().copied().map(|i| i as u16).unwrap_or(NONE),
            ..Node::EMPTY
        };
        n.role = catalog::component(tag).expect("validated component").role;
        if kind == Kind::Popup {
            // Anchored at the viewport origin until the host sets its point.
            n.set_popup_anchor(0, 0);
        }
        if n.parent != NONE && !view.nodes[n.parent as usize].kind.container() {
            return Err(p.error("leaf element cannot have children"));
        }
        let mut attrs: Vec<String> = Vec::new();
        let mut m = Meta {
            tag: tag.into(),
            id: String::new(),
            classes: Vec::new(),
        };
        if kind == Kind::VirtualList {
            if view.list_len as usize == virtual_list::MAX_LISTS {
                return Err(p.error("virtual-list limit exceeded"));
            }
            n.value = view.list_len;
            view.list_len += 1;
            view.lists[n.value as usize].item_height = 36;
            view.lists[n.value as usize].overscan = 1;
            n.style.overflow = 2;
            n.narrow.overflow = 2;
        }
        // `ui-icon src` names a host image; the name is the icon's text.
        let mut src: Option<String> = None;
        if kind == Kind::Template {
            if n.parent == NONE || view.nodes[n.parent as usize].kind != Kind::VirtualList {
                return Err(p.error("template must be a direct child of ui-virtual-list"));
            }
            m.id = format!("{}.template", meta[n.parent as usize].id);
            n.id = names.intern(&m.id, &p)?;
        }
        loop {
            p.ws();
            if p.take(">") {
                break;
            }
            let attr = p.name()?;
            if attrs.iter().any(|a| a == attr) {
                return Err(p.error("duplicate attribute"));
            }
            if attrs.len() == 12 {
                return Err(p.error("attribute limit exceeded"));
            }
            attrs.push(attr.into());
            p.ws();
            let value = if matches!(attr, "hidden" | "disabled") && !p.rest().starts_with('=') {
                String::new()
            } else {
                p.expect("=")?;
                p.ws();
                p.quoted()?
            };
            match attr {
                "src" if kind == Kind::Icon => {
                    if value.is_empty()
                        || value.len() > 64
                        || !value
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
                    {
                        return Err(p.error("src requires a 1-64 character image name"));
                    }
                    src = Some(value);
                }
                "source" if kind == Kind::VirtualList => {
                    view.lists[n.value as usize].source = names.intern(&value, &p)?
                }
                "item-height" if kind == Kind::VirtualList => {
                    view.lists[n.value as usize].item_height = value
                        .parse()
                        .ok()
                        .filter(|v| (1..=4096).contains(v))
                        .ok_or_else(|| p.error("item-height requires 1..4096 pixels"))?;
                }
                "overscan" if kind == Kind::VirtualList => {
                    view.lists[n.value as usize].overscan = value
                        .parse()
                        .ok()
                        .filter(|v| *v <= 8)
                        .ok_or_else(|| p.error("overscan requires 0..8 rows"))?;
                }
                "id" => {
                    n.id = names.intern(&value, &p)?;
                    m.id = value;
                }
                "class" => {
                    for c in value.split_ascii_whitespace() {
                        names.intern(c, &p)?;
                        m.classes.push(c.into());
                    }
                    if m.classes.len() > 8 {
                        return Err(p.error("class limit exceeded"));
                    }
                }
                "action" => {
                    if !catalog::actionable(kind) {
                        return Err(
                            p.error("actions require a button, selectable row or text input")
                        );
                    }
                    n.action = names.intern(&value, &p)?;
                }
                _ if catalog::binding(attr).is_some() => {
                    let binding = catalog::binding(attr).expect("known binding");
                    if !binding.accepts(kind) {
                        return Err(p.error("binding type is incompatible with element"));
                    }
                    n.bindings[binding.slot] = names.intern(&value, &p)?;
                }
                "disabled" | "hidden" => {
                    // HTML boolean attributes are true by presence, including
                    // disabled="false". Bindings supply dynamic booleans.
                    n.state |= if attr == "hidden" {
                        state::HIDDEN
                    } else {
                        state::DISABLED
                    };
                }
                _ => return Err(p.error(format!("unsupported attribute '{attr}'"))),
            }
        }
        if n.id == 0 {
            return Err(p.error("every element requires a stable id"));
        }
        if view.nodes[..i].iter().any(|old| old.id == n.id) {
            return Err(p.error("duplicate element id"));
        }
        n.text_start = view.text_len;
        if !kind.container() {
            let end = p
                .rest()
                .find('<')
                .ok_or_else(|| p.error("missing closing element"))?;
            let text =
                entities(p.rest()[..end].trim()).ok_or_else(|| p.error("unsupported entity"))?;
            let text = match src.take() {
                Some(_) if !text.is_empty() => {
                    return Err(p.error("an icon with src has no text"));
                }
                Some(name) => name,
                None => text,
            };
            if text.len() > MAX_LEAF_TEXT || view.text_len as usize + text.len() > MAX_TEXT {
                return Err(p.error("text limit exceeded"));
            }
            n.text_len = text.len() as u16;
            view.text[view.text_len as usize..view.text_len as usize + text.len()]
                .copy_from_slice(text.as_bytes());
            view.text_len += text.len() as u16;
            p.pos += end;
        }
        view.nodes[i] = n;
        view.len += 1;
        meta.push(m);
        stack.push(i);
    }
    if !stack.is_empty() {
        return Err(p.error("unclosed element"));
    }
    view.validate()
        .map_err(|e| p.error(format!("invalid component contract: {e:?}")))?;
    styles(&mut view, &meta, css, defaults)?;
    // A popup's vertical scroll field holds its anchor; it never scrolls.
    if let Some(i) = (0..view.len as usize).find(|i| {
        let n = &view.nodes[*i];
        n.kind == Kind::Popup && (n.style.overflow == 2 || n.narrow.overflow == 2)
    }) {
        return Err(p.error(format!(
            "ui-popup-menu cannot scroll (overflow: auto on #{})",
            meta[i].id
        )));
    }
    view.validate()
        .map_err(|e| p.error(format!("invalid styled view: {e:?}")))?;
    Ok(view)
}

#[derive(Clone)]
struct Rule {
    selector: String,
    states: u8,
    media: u16,
    declarations: Vec<(String, String, usize)>,
    order: u32,
}
fn declarations(p: &mut Parser<'_>) -> Result<Vec<(String, String, usize)>> {
    p.expect("{")?;
    let mut result = Vec::new();
    loop {
        p.ws();
        if p.take("}") {
            return Ok(result);
        }
        if result.len() == 16 {
            return Err(p.error("declaration limit exceeded"));
        }
        // CSS property names do not include the ':' delimiter.
        let offset = p.pos;
        let end = p
            .rest()
            .find(':')
            .ok_or_else(|| p.error("expected property: value"))?;
        let property = p.rest()[..end].trim();
        if property.is_empty()
            || !property
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'-')
        {
            return Err(p.error("invalid property name"));
        }
        p.pos += end + 1;
        let end = p
            .rest()
            .find([';', '}'])
            .ok_or_else(|| p.error("unclosed declaration"))?;
        let value = p.rest()[..end].trim();
        if value.is_empty() {
            return Err(p.error("empty property value"));
        }
        result.push((property.into(), value.into(), offset));
        p.pos += end;
        p.take(";");
    }
}
fn css_rules(
    p: &mut Parser<'_>,
    rules: &mut Vec<Rule>,
    vars: &mut Vec<(String, String)>,
    media: u16,
) -> Result<()> {
    loop {
        p.ws();
        if p.rest().is_empty() {
            return if media == 0 {
                Ok(())
            } else {
                Err(p.error("unclosed media rule"))
            };
        }
        if media != 0 && p.take("}") {
            return Ok(());
        }
        if p.take("/*") {
            let end = p
                .rest()
                .find("*/")
                .ok_or_else(|| p.error("unclosed CSS comment"))?;
            p.pos += end + 2;
            continue;
        }
        if p.take("@media") {
            if media != 0 {
                return Err(p.error("nested media rules are unsupported"));
            }
            p.ws();
            p.expect("(")?;
            p.ws();
            p.expect("max-width")?;
            p.ws();
            p.expect(":")?;
            p.ws();
            let end = p
                .rest()
                .find(')')
                .ok_or_else(|| p.error("unclosed media condition"))?;
            let threshold = pixels(p.rest()[..end].trim())
                .filter(|v| *v > 0 && *v <= 4096)
                .ok_or_else(|| p.error("media width must be 1..4096px"))?;
            p.pos += end + 1;
            p.ws();
            p.expect("{")?;
            css_rules(p, rules, vars, threshold)?;
            continue;
        }
        let end = p
            .rest()
            .find('{')
            .ok_or_else(|| p.error("expected style rule"))?;
        let selector_offset = p.pos;
        let selector = p.rest()[..end].trim().to_string();
        p.pos += end;
        let decls = declarations(p)?;
        if selector == ":root" {
            if media != 0 {
                return Err(p.error("theme tokens cannot be responsive"));
            }
            for (name, value, _) in decls {
                if !name.starts_with("--") || vars.len() == 32 {
                    return Err(p.error("expected bounded theme token"));
                }
                if let Some(old) = vars.iter_mut().find(|(n, _)| *n == name) {
                    old.1 = value;
                } else {
                    vars.push((name, value));
                }
            }
        } else {
            let after_rule = p.pos;
            p.pos = selector_offset;
            let mut parts = selector.split(':');
            let base = parts.next().unwrap_or("");
            let name = base.strip_prefix(['.', '#']).unwrap_or(base);
            let mut states = 0;
            for suffix in parts {
                let flag = match suffix {
                    "focus" => state::FOCUSED,
                    "disabled" => state::DISABLED,
                    "selected" => state::SELECTED,
                    _ => return Err(p.error(format!("unsupported state selector ':{suffix}'"))),
                };
                if states & flag != 0 {
                    return Err(p.error("duplicate state selector"));
                }
                states |= flag;
            }
            if (name.is_empty() && (states == 0 || !base.is_empty()))
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
            {
                return Err(p.error("expected type, .class or #id with optional state suffixes"));
            }
            let selector = base.to_string();
            if rules.len() == 128 {
                return Err(p.error("style rule limit exceeded"));
            }
            rules.push(Rule {
                selector,
                states,
                media,
                declarations: decls,
                order: rules.len() as u32 + 1,
            });
            p.pos = after_rule;
        }
    }
}
fn pixels(value: &str) -> Option<u16> {
    value
        .strip_suffix("px")
        .unwrap_or(if value == "0" { "0" } else { "" })
        .parse()
        .ok()
}
fn resolve<'a>(mut value: &'a str, vars: &'a [(String, String)]) -> Option<&'a str> {
    for _ in 0..8 {
        if let Some(name) = value.strip_prefix("var(").and_then(|v| v.strip_suffix(')')) {
            value = &vars.iter().find(|(n, _)| n == name.trim())?.1;
        } else {
            return Some(value);
        }
    }
    None
}
fn matches(rule: &Rule, m: &Meta) -> bool {
    if rule.selector.is_empty() {
        return true;
    }
    if let Some(id) = rule.selector.strip_prefix('#') {
        id == m.id
    } else if let Some(class) = rule.selector.strip_prefix('.') {
        m.classes.iter().any(|c| c == class)
    } else {
        rule.selector == m.tag
    }
}
/// One node's cascade for one state combination. The decoration is interned
/// into the document table once the cascade is complete.
#[derive(Clone, Copy)]
struct Computed {
    style: Style,
    decoration: Decoration,
}
impl Computed {
    const DEFAULT: Self = Self {
        style: Style::DEFAULT,
        decoration: Decoration::NONE,
    };
}
/// CSS border-width with one to four values (top, right, bottom, left).
/// Drawn sides share one width; every other side is zero.
fn border_widths(value: &str) -> Option<(u8, u8)> {
    let mut values = [0u16; 4];
    let mut count = 0;
    for part in value.split_ascii_whitespace() {
        *values.get_mut(count)? = pixels(part)?;
        count += 1;
    }
    let sides = match count {
        1 => [values[0]; 4],
        2 => [values[0], values[1], values[0], values[1]],
        3 => [values[0], values[1], values[2], values[1]],
        4 => values,
        _ => return None,
    };
    let (mut width, mut mask) = (0, 0);
    for (side, side_width) in sides.into_iter().enumerate() {
        if side_width == 0 {
            continue;
        }
        if width != 0 && side_width != width {
            return None;
        }
        width = side_width;
        mask |= 1 << side;
    }
    Some((u8::try_from(width).ok()?, mask))
}
/// SVG fill-opacity as coverage: 0..1 with at most three decimals, or a
/// whole percentage.
fn opacity(value: &str) -> Option<u8> {
    let digits = |text: &str| text.bytes().all(|b| b.is_ascii_digit());
    let thousandths = if let Some(percent) = value.strip_suffix('%') {
        if percent.is_empty() || !digits(percent) {
            return None;
        }
        percent.parse::<u32>().ok()?.checked_mul(10)?
    } else {
        let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
        if whole.len() > 1
            || fraction.len() > 3
            || (whole.is_empty() && fraction.is_empty())
            || !digits(whole)
            || !digits(fraction)
        {
            return None;
        }
        let mut thousandths = whole.parse::<u32>().unwrap_or(0) * 1000;
        for (place, digit) in [100, 10, 1].into_iter().zip(fraction.bytes()) {
            thousandths += place * u32::from(digit - b'0');
        }
        thousandths
    };
    (thousandths <= 1000).then(|| ((thousandths * 255 + 500) / 1000) as u8)
}
fn property(computed: &mut Computed, name: &str, value: &str) -> Option<usize> {
    let Computed {
        style: s,
        decoration: d,
    } = computed;
    let slot = match name {
        "width" => {
            s.width = if value == "auto" {
                i16::MAX
            } else if let Some(v) = value.strip_suffix('%') {
                let v: i16 = v.parse().ok()?;
                if !(1..=100).contains(&v) {
                    return None;
                }
                -v
            } else {
                i16::try_from(pixels(value)?).ok()?
            };
            0
        }
        "height" => {
            s.height = if value == "auto" {
                u16::MAX
            } else {
                pixels(value)?
            };
            1
        }
        "padding" => {
            s.padding = pixels(value)?;
            2
        }
        "gap" => {
            s.gap = pixels(value)?;
            3
        }
        "font-size" => {
            s.font_size = pixels(value)?;
            4
        }
        "border-color" if value.eq_ignore_ascii_case("currentcolor") => {
            d.border_current_color = true;
            d.border_color = 0;
            d.border_token = 0;
            15
        }
        "color" | "background-color" | "border-color" => {
            let token = theme::token(value).unwrap_or(0);
            let (c, argb) = if token != 0 {
                let argb = theme::Theme::Dark.color(token, 0);
                (argb & 0x00ff_ffff, argb)
            } else {
                let hex = value.strip_prefix('#')?;
                if hex.len() != 6 && !(name != "color" && hex.len() == 8) {
                    return None;
                }
                let c = u32::from_str_radix(hex, 16).ok()?;
                (
                    c,
                    if hex.len() == 8 {
                        c.rotate_right(8)
                    } else {
                        c | 0xff00_0000
                    },
                )
            };
            match name {
                "color" => {
                    s.color = c;
                    s.color_token = token;
                    5
                }
                "background-color" => {
                    s.background = argb;
                    s.background_token = token;
                    s.fill = true;
                    6
                }
                _ => {
                    d.border_color = argb;
                    d.border_token = token;
                    d.border_current_color = false;
                    15
                }
            }
        }
        "flex-direction" => {
            s.direction = match value {
                "row" => 1,
                "column" => 0,
                _ => return None,
            };
            7
        }
        "overflow" => {
            s.overflow = match value {
                "visible" => 0,
                "hidden" => 1,
                "auto" => 2,
                _ => return None,
            };
            8
        }
        "display" => {
            if !matches!(value, "flex" | "block") {
                return None;
            }
            9
        }
        "flex-grow" => {
            s.grow = match value {
                "0" => false,
                "1" => true,
                _ => return None,
            };
            10
        }
        "text-align" => {
            s.text_align = match value {
                "left" => 0,
                "center" => 1,
                "right" => 2,
                _ => return None,
            };
            11
        }
        "vertical-align" => {
            d.vertical_align = match value {
                "top" => 0,
                "middle" => 1,
                "bottom" => 2,
                _ => return None,
            };
            12
        }
        "border-radius" => {
            d.radius = u8::try_from(pixels(value)?).ok()?;
            13
        }
        "border-width" => {
            (d.border_width, d.border_sides) = border_widths(value)?;
            14
        }
        "fill-opacity" => {
            d.fill_opacity = opacity(value)?;
            16
        }
        "align-items" => {
            d.align_items = match value {
                "flex-start" | "start" => 0,
                "center" => 1,
                "flex-end" | "end" => 2,
                "stretch" => 3,
                _ => return None,
            };
            17
        }
        "outline" => {
            d.outline_none = match value {
                "none" => true,
                "auto" => false,
                _ => return None,
            };
            18
        }
        // `none`, a shared background preset `theme(dusk)`, or the host's
        // image set `image(0)`.
        "background-image" => {
            d.background_image = if value == "none" {
                0
            } else if let Some(index) = value.strip_prefix("image(") {
                let index: u8 = index.strip_suffix(')')?.trim().parse().ok()?;
                if index >= HOST_IMAGES {
                    return None;
                }
                HOST_IMAGE + index
            } else {
                let name = value.strip_prefix("theme(")?.strip_suffix(')')?.trim();
                let preset = appearance::BACKGROUNDS
                    .iter()
                    .position(|background| background.name == name)?;
                preset as u8 + 1
            };
            19
        }
        _ => return None,
    };
    (s.valid() && d.valid()).then_some(slot)
}
fn styles(view: &mut Document, meta: &[Meta], css: &str, base: &str) -> Result<()> {
    let mut p = Parser {
        source: "styles",
        text: css,
        pos: 0,
    };
    if css.len() > 16384 {
        return Err(p.error("styles exceed 16 KiB"));
    }
    let mut rules = Vec::new();
    let mut vars = Vec::new();
    let mut defaults = Parser {
        source: "SDK controls",
        text: base,
        pos: 0,
    };
    css_rules(&mut defaults, &mut rules, &mut vars, 0)?;
    let base_count = rules.len();
    css_rules(&mut p, &mut rules, &mut vars, 0)?;
    if rules.len() - base_count > 96 {
        return Err(p.error("application style rule limit exceeded"));
    }
    let threshold = rules.iter().find(|r| r.media != 0).map_or(0, |r| r.media);
    if rules.iter().any(|r| r.media != 0 && r.media != threshold) {
        return Err(p.error("v1 supports one responsive breakpoint per view"));
    }
    // Validate even unmatched declarations; unsupported CSS never disappears.
    for rule in &rules {
        for (key, value, offset) in &rule.declarations {
            p.pos = *offset;
            let value =
                resolve(value, &vars).ok_or_else(|| p.error("missing/cyclic theme token"))?;
            let mut checked_style = Computed::DEFAULT;
            property(&mut checked_style, key, value).ok_or_else(|| {
                if key == "font-size" {
                    let sizes: Vec<String> = crate::text_atlas::UI_TEXT_SIZES
                        .iter()
                        .map(|size| format!("{size}px"))
                        .collect();
                    p.error(format!(
                        "font-size '{value}' has no text table; use one of {}",
                        sizes.join(", ")
                    ))
                } else {
                    p.error(format!("unsupported property/value '{key}: {value}'"))
                }
            })?;
        }
    }
    for i in 0..view.len as usize {
        for narrow in [false, true] {
            for combination in 0..8 {
                let active_states = (combination as u8) << 1;
                let mut style = Computed::DEFAULT;
                if view.nodes[i].parent != NONE {
                    let parent = &view.nodes[view.nodes[i].parent as usize];
                    let inherited = if narrow { parent.narrow } else { parent.style };
                    style.style.color = inherited.color;
                    style.style.color_token = inherited.color_token;
                    style.style.font_size = inherited.font_size;
                    style.style.text_align = inherited.text_align;
                }
                if view.nodes[i].kind == Kind::Row {
                    style.style.direction = 1;
                }
                if view.nodes[i].kind == Kind::VirtualList {
                    style.style.overflow = 2;
                    style.style.grow = true;
                }
                let mut ranks = [0; 20];
                let mut align_items_at = 0;
                let mut display = "block";
                for rule in &rules {
                    if (!narrow && rule.media != 0)
                        || !matches(rule, &meta[i])
                        || active_states & rule.states != rule.states
                    {
                        continue;
                    }
                    // CSS specificity: IDs outrank classes/pseudo-classes,
                    // which outrank types. Source order breaks ties per property.
                    let specificity = if rule.selector.starts_with('#') {
                        10000
                    } else if rule.selector.starts_with('.') {
                        100
                    } else if rule.selector.is_empty() {
                        0
                    } else {
                        1
                    };
                    // Application CSS is a separate, higher cascade origin. Even a
                    // type rule can resize/redecorate a standard SDK class.
                    let origin = if rule.order as usize > base_count {
                        1 << 30
                    } else {
                        0
                    };
                    let rank =
                        origin + (specificity + rule.states.count_ones() * 100) * 1000 + rule.order;
                    for (key, value, offset) in &rule.declarations {
                        p.pos = *offset;
                        let value = resolve(value, &vars)
                            .ok_or_else(|| p.error("missing/cyclic theme token"))?;
                        let mut candidate = style;
                        let slot = property(&mut candidate, key, value)
                            .ok_or_else(|| p.error("invalid declaration"))?;
                        if rank >= ranks[slot] {
                            style = candidate;
                            ranks[slot] = rank;
                            if key == "display" {
                                display = value;
                            }
                            if slot == 17 {
                                align_items_at = *offset;
                            }
                        }
                    }
                }
                if ranks[9] != 0 && display == "block" {
                    style.style.direction = 0;
                }
                // Column children span the width (start/stretch); centring
                // them would need intrinsic widths the layout does not have.
                if style.style.direction == 0 && matches!(style.decoration.align_items, 1 | 2) {
                    p.pos = align_items_at;
                    return Err(p.error(format!(
                        "align-items center/end requires a flex row (#{})",
                        meta[i].id
                    )));
                }
                // CSS: an undeclared border color is the text color.
                if ranks[15] == 0 {
                    style.decoration.border_current_color = true;
                }
                let decoration = style.decoration.canonical();
                let mut style = style.style;
                if decoration != Decoration::NONE {
                    let len = view.decoration_len as usize;
                    let index = match view.decorations[..len]
                        .iter()
                        .position(|d| *d == decoration)
                    {
                        Some(index) => index,
                        None if len == MAX_DECORATIONS => {
                            return Err(p.error(format!(
                                "decoration table exceeds {MAX_DECORATIONS} entries"
                            )));
                        }
                        None => {
                            view.decorations[len] = decoration;
                            view.decoration_len += 1;
                            len
                        }
                    };
                    style.decoration = index as u8;
                }
                let range = usize::from(narrow);
                view.nodes[i].inherited[range * 8 + combination] = u8::from(ranks[5] == 0)
                    | (u8::from(ranks[4] == 0) << 1)
                    | (u8::from(ranks[11] == 0) << 2);
                if combination == 0 {
                    if narrow {
                        view.nodes[i].narrow = style;
                    } else {
                        view.nodes[i].style = style;
                    }
                } else {
                    let base = if narrow {
                        view.nodes[i].narrow
                    } else {
                        view.nodes[i].style
                    };
                    if style != base {
                        let index = if let Some(index) = view.state_styles
                            [..view.state_style_len as usize]
                            .iter()
                            .position(|s| *s == style)
                        {
                            index
                        } else {
                            let index = view.state_style_len as usize;
                            if index == MAX_STATE_STYLES {
                                return Err(p.error("state style palette exceeds 128 entries"));
                            }
                            view.state_styles[index] = style;
                            view.state_style_len += 1;
                            index
                        };
                        view.nodes[i].state_styles[range * 7 + combination - 1] = index as u8;
                    }
                }
            }
        }
        view.nodes[i].breakpoint = threshold;
    }
    Ok(())
}
