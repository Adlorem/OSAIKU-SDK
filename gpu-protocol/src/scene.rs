//! Common immutable batch envelope used by the CPU broker. Hardware execution
//! stays in the family package; compositor authority is checked separately.
use crate::{
    buffer::Handle,
    compositor,
    render::{self, Error, Scope},
};
#[derive(Clone, Copy)]
pub enum Batch<'a> {
    Painter(render::Batch<'a>),
    Compositor(compositor::Batch<'a>),
}
impl<'a> Batch<'a> {
    pub fn decode(bytes: &'a [u8], scope: Scope) -> Result<Self, Error> {
        if bytes.starts_with(b"OSAGCB01") {
            compositor::Batch::decode(bytes, scope).map(Self::Compositor)
        } else {
            render::Batch::decode(bytes, scope).map(Self::Painter)
        }
    }
    pub fn viewport(self) -> (u32, u32) {
        match self {
            Self::Painter(b) => b.viewport(),
            Self::Compositor(b) => b.viewport(),
        }
    }
    pub fn scope(self) -> Scope {
        match self {
            Self::Painter(b) => b.scope(),
            Self::Compositor(b) => b.scope(),
        }
    }
    pub fn len(self) -> usize {
        match self {
            Self::Painter(b) => b.len(),
            Self::Compositor(b) => b.len(),
        }
    }
    pub fn compositor(self) -> bool {
        matches!(self, Self::Compositor(_))
    }
    pub fn asset(self, n: usize) -> Result<Option<(Handle, u64, u64)>, Error> {
        let (h, o, l) = match self {
            Self::Painter(b) => {
                let d = b.draw(n).ok_or(Error::Length)??;
                (d.asset, d.offset, d.length)
            }
            Self::Compositor(b) => {
                let c = b.command(n).ok_or(Error::Length)?;
                (c.asset, c.offset, c.length)
            }
        };
        Ok((l != 0).then_some((h, o, l)))
    }
    pub fn control(self) -> Option<u32> {
        match self {
            Self::Painter(b) => b.control(),
            Self::Compositor(b) => {
                let c = b.command(0)?;
                (b.len() == 1).then_some(100 + c.op)
            }
        }
    }
    pub fn permits_result(self) -> bool {
        matches!(
            self.control(),
            Some(2 | 10 | 101 | 110 | 115 | 116 | 117 | 118)
        )
    }
}
