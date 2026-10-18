use crate::*;

struct Writer<'a> {
    out: &'a mut [u8],
    pos: usize,
}
impl Writer<'_> {
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let end = self.pos.checked_add(bytes.len()).ok_or(Error::Buffer)?;
        self.out
            .get_mut(self.pos..end)
            .ok_or(Error::Buffer)?
            .copy_from_slice(bytes);
        self.pos = end;
        Ok(())
    }
    fn u8(&mut self, v: u8) -> Result<(), Error> {
        self.bytes(&[v])
    }
    fn u16(&mut self, v: u16) -> Result<(), Error> {
        self.bytes(&v.to_le_bytes())
    }
    fn u32(&mut self, v: u32) -> Result<(), Error> {
        self.bytes(&v.to_le_bytes())
    }
    fn style(&mut self, s: Style) -> Result<(), Error> {
        for v in [s.width as u16, s.height, s.padding, s.gap, s.font_size] {
            self.u16(v)?;
        }
        self.u32(s.color)?;
        self.u32(s.background)?;
        self.u8(s.color_token)?;
        self.u8(s.background_token)?;
        self.bytes(&[
            s.direction,
            s.overflow,
            u8::from(s.fill),
            u8::from(s.grow),
            s.text_align,
            s.decoration,
        ])
    }
    fn decoration(&mut self, d: Decoration) -> Result<(), Error> {
        self.bytes(&[
            d.radius,
            d.border_width,
            d.border_sides,
            d.vertical_align,
            d.align_items,
            d.fill_opacity,
            u8::from(d.border_current_color),
        ])?;
        self.u32(d.border_color)?;
        self.bytes(&[d.border_token, u8::from(d.outline_none)])
    }
}
fn flag(value: u8) -> Result<bool, Error> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::Structure),
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn bytes(&mut self, len: usize) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(len).ok_or(Error::Buffer)?;
        let result = self.bytes.get(self.pos..end).ok_or(Error::Buffer)?;
        self.pos = end;
        Ok(result)
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(
            self.bytes(2)?.try_into().map_err(|_| Error::Buffer)?,
        ))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?.try_into().map_err(|_| Error::Buffer)?,
        ))
    }
    fn style(&mut self) -> Result<Style, Error> {
        let s = Style {
            width: self.u16()? as i16,
            height: self.u16()?,
            padding: self.u16()?,
            gap: self.u16()?,
            font_size: self.u16()?,
            color: self.u32()?,
            background: self.u32()?,
            color_token: self.u8()?,
            background_token: self.u8()?,
            direction: self.u8()?,
            overflow: self.u8()?,
            fill: flag(self.u8()?)?,
            grow: flag(self.u8()?)?,
            text_align: self.u8()?,
            decoration: self.u8()?,
        };
        if !s.valid() {
            return Err(Error::Structure);
        }
        Ok(s)
    }
    fn decoration(&mut self) -> Result<Decoration, Error> {
        Ok(Decoration {
            radius: self.u8()?,
            border_width: self.u8()?,
            border_sides: self.u8()?,
            vertical_align: self.u8()?,
            align_items: self.u8()?,
            fill_opacity: self.u8()?,
            border_current_color: flag(self.u8()?)?,
            border_color: self.u32()?,
            border_token: self.u8()?,
            outline_none: flag(self.u8()?)?,
        })
    }
}
impl Document {
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        let mut w = Writer { out, pos: 0 };
        w.bytes(b"OSUI")?;
        w.u16(VERSION)?;
        w.u16(self.len)?;
        w.u16(self.text_len)?;
        w.u16(self.point_len)?;
        w.u16(self.state_style_len)?;
        w.u16(self.decoration_len)?;
        for n in &self.nodes[..self.len as usize] {
            w.u32(n.id)?;
            w.u16(n.parent)?;
            w.u8(n.kind as u8)?;
            w.u8(n.role as u8)?;
            w.u8(n.state)?;
            w.u32(n.action)?;
            w.u16(n.text_start)?;
            w.u16(n.text_len)?;
            for b in n.bindings {
                w.u32(b)?;
            }
            w.u16(n.value)?;
            w.u16(n.series_start)?;
            w.u16(n.series_len)?;
            w.style(n.style)?;
            w.style(n.narrow)?;
            w.u16(n.breakpoint)?;
            w.u32(n.scroll_y)?;
            if n.kind == Kind::VirtualList {
                w.u32(self.lists[n.value as usize].source)?;
                w.u16(self.lists[n.value as usize].item_height)?;
                w.u16(self.lists[n.value as usize].overscan)?;
                w.u32(self.lists[n.value as usize].total)?;
                w.u32(self.lists[n.value as usize].first)?;
            }
            w.bytes(&n.state_styles)?;
            w.bytes(&n.inherited)?;
        }
        w.bytes(&self.text[..self.text_len as usize])?;
        for p in &self.points[..self.point_len as usize] {
            w.bytes(&p.time_ms.to_le_bytes())?;
            w.u16(p.value)?;
        }
        for style in &self.state_styles[..self.state_style_len as usize] {
            w.style(*style)?;
        }
        for decoration in &self.decorations[..self.decoration_len as usize] {
            w.decoration(*decoration)?;
        }
        if w.pos > MAX_WIRE {
            return Err(Error::Limit);
        }
        Ok(w.pos)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut doc = Self::EMPTY;
        Self::decode_into(bytes, &mut doc)?;
        Ok(doc)
    }
    /// Decode into a separate admitted scratch buffer, then publish only on
    /// success. Avoids returning a large aggregate on a freestanding stack.
    pub fn decode_into(bytes: &[u8], doc: &mut Self) -> Result<(), Error> {
        if bytes.len() > MAX_WIRE {
            return Err(Error::Limit);
        }
        let mut r = Reader { bytes, pos: 0 };
        if r.bytes(4)? != b"OSUI" || r.u16()? != VERSION {
            return Err(Error::Version);
        }
        doc.nodes.fill(Node::EMPTY);
        doc.text.fill(0);
        doc.points.fill(Point::EMPTY);
        doc.state_styles.fill(Style::DEFAULT);
        doc.decorations.fill(Decoration::NONE);
        doc.lists.fill(virtual_list::Config::EMPTY);
        doc.list_len = 0;
        doc.len = r.u16()?;
        doc.text_len = r.u16()?;
        doc.point_len = r.u16()?;
        doc.state_style_len = r.u16()?;
        doc.decoration_len = r.u16()?;
        if doc.len as usize > MAX_NODES
            || doc.text_len as usize > MAX_TEXT
            || doc.point_len as usize > MAX_POINTS
            || doc.state_style_len as usize > MAX_STATE_STYLES
            || doc.decoration_len as usize > MAX_DECORATIONS
        {
            return Err(Error::Limit);
        }
        for n in &mut doc.nodes[..doc.len as usize] {
            n.id = r.u32()?;
            n.parent = r.u16()?;
            n.kind = Kind::from_byte(r.u8()?).ok_or(Error::Structure)?;
            n.role = Role::from_byte(r.u8()?).ok_or(Error::Structure)?;
            n.state = r.u8()?;
            n.action = r.u32()?;
            n.text_start = r.u16()?;
            n.text_len = r.u16()?;
            for b in &mut n.bindings {
                *b = r.u32()?;
            }
            n.value = r.u16()?;
            n.series_start = r.u16()?;
            n.series_len = r.u16()?;
            n.style = r.style()?;
            n.narrow = r.style()?;
            n.breakpoint = r.u16()?;
            n.scroll_y = r.u32()?;
            if n.kind == Kind::VirtualList {
                if n.value != doc.list_len || doc.list_len as usize == virtual_list::MAX_LISTS {
                    return Err(Error::Structure);
                }
                doc.list_len += 1;
                doc.lists[n.value as usize] = virtual_list::Config {
                    source: r.u32()?,
                    item_height: r.u16()?,
                    overscan: r.u16()?,
                    total: r.u32()?,
                    first: r.u32()?,
                };
            }
            n.state_styles.copy_from_slice(r.bytes(14)?);
            n.inherited.copy_from_slice(r.bytes(16)?);
        }
        doc.text[..doc.text_len as usize].copy_from_slice(r.bytes(doc.text_len as usize)?);
        for p in &mut doc.points[..doc.point_len as usize] {
            p.time_ms = u64::from_le_bytes(r.bytes(8)?.try_into().map_err(|_| Error::Buffer)?);
            p.value = r.u16()?;
        }
        for style in &mut doc.state_styles[..doc.state_style_len as usize] {
            *style = r.style()?;
        }
        for decoration in &mut doc.decorations[..doc.decoration_len as usize] {
            *decoration = r.decoration()?;
        }
        if r.pos != bytes.len() {
            return Err(Error::Buffer);
        }
        doc.validate()?;
        Ok(())
    }
}
