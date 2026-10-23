#![no_std]
#![no_main]

use core::fmt::{self, Write};
use osaiku_runtime::packaged::PackagedView;
use osaiku_runtime::{
    hosted::{self, WindowApplication},
    process_exit,
};
use osaiku_ui::{input, symbol, Bindings, Error, Value};

static VIEW: PackagedView = PackagedView::new();
const TEMPLATE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hello.ui.bin"));

struct Counter {
    count: u32,
    focus: u32,
    accepted: [osaiku_ui::Identity; 2],
}
impl WindowApplication for Counter {
    // Kernel-issued generic launch context; no built-in app ID or slot.
    fn new() -> Self {
        Self {
            count: 0,
            focus: 0,
            accepted: [osaiku_ui::Action::EMPTY.target; 2],
        }
    }
    fn submitted(&mut self, identity: osaiku_ui::Identity) {
        self.accepted = [identity, self.accepted[0]];
    }
    fn event(&mut self, event: &input::Event) -> bool {
        if event.target.owner == 0 || !self.accepted.contains(&event.target) {
            return false;
        }
        if event.kind == input::FOCUS {
            self.focus = event.element;
            return true;
        }
        if event.kind != input::ACTIVATE {
            return false;
        }
        if event.element == symbol("increment") && event.action == symbol("increment") {
            self.count = self.count.saturating_add(1);
        } else if event.element == symbol("reset") && event.action == symbol("reset") {
            self.count = 0;
        } else {
            return false;
        }
        self.focus = event.element;
        true
    }
    fn render(&mut self, output: &mut [u8]) -> Result<usize, Error> {
        let mut text = Text {
            bytes: [0; 32],
            len: 0,
        };
        write!(&mut text, "Licznik: {}", self.count).map_err(|_| Error::Limit)?;
        let model = Model {
            text: core::str::from_utf8(&text.bytes[..text.len]).map_err(|_| Error::Text)?,
            empty: self.count == 0,
        };
        VIEW.encode(TEMPLATE, &model, self.focus, output)
    }
    fn prepare_shutdown(&mut self) -> bool {
        true
    } // Only a disposable counter.
}
struct Model<'a> {
    text: &'a str,
    empty: bool,
}
impl Bindings for Model<'_> {
    fn get(&self, name: u32) -> Option<Value<'_>> {
        if name == symbol("count") {
            Some(Value::Text(self.text))
        } else if name == symbol("empty") {
            Some(Value::Bool(self.empty))
        } else {
            None
        }
    }
}
struct Text {
    bytes: [u8; 32],
    len: usize,
}
impl Write for Text {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.len + value.len() > self.bytes.len() {
            return Err(fmt::Error);
        }
        self.bytes[self.len..self.len + value.len()].copy_from_slice(value.as_bytes());
        self.len += value.len();
        Ok(())
    }
}
#[no_mangle]
pub extern "C" fn _start() -> ! {
    #[cfg(feature = "surfaces")]
    hosted::run_with_surfaces::<Counter>();
    #[cfg(not(feature = "surfaces"))]
    hosted::run::<Counter>()
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    process_exit(30)
}
