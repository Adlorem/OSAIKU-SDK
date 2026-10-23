//! Reusable message with an explicit dismiss action. The host owns its lifetime;
//! rendering a notice never grants the subject application any authority.
use crate::{
    symbol,
    text_model::{Model, Text},
};
use core::fmt::{self, Write};

pub const HTML: &str = include_str!("notice.ui.html");
pub const CSS: &str = include_str!("notice.ui.css");
pub const DISMISS: u32 = symbol("notice-dismiss");

pub struct Message<'a> {
    pub title: &'a str,
    pub message: &'a str,
    pub detail: &'a str,
    pub dismiss: &'a str,
}
impl Model for Message<'_> {
    fn flag(&self, _: u32) -> Option<bool> {
        None
    }
    fn text(&self, key: u32, out: &mut Text) -> fmt::Result {
        out.write_str(match key {
            k if k == symbol("notice-title") => self.title,
            k if k == symbol("notice-message") => self.message,
            k if k == symbol("notice-detail") => self.detail,
            DISMISS => self.dismiss,
            _ => return Err(fmt::Error),
        })
    }
}
