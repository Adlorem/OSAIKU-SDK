#![no_std]
#![no_main]
use core::fmt::{self, Write};
use osaiku_runtime::{
    hosted::{self, WindowApplication},
    report::{Prepared, ReportView, Session, Surface},
};
use osaiku_ui::{catalog::ActionKind, input, symbol, Bindings, Error, Identity, Value};
#[path = "../contract.rs"]
mod contract;
static REPORT: ReportView = ReportView::new();
const TEMPLATE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/report.ui.bin"));
const SURROUNDING: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/region.ui.bin"));

struct Report {
    session: Session,
    pending: Option<Prepared>,
    sample: u32,
    embedded: bool,
}
impl WindowApplication for Report {
    // Like window-hello, deployment uses the admitted demo image slot in a
    // separate ESP. This example registers no app and grants no update access.
    const APP_ID: u64 = 7;
    fn new() -> Self {
        let owner = osaiku_runtime::process_self().unwrap().encode();
        Self {
            session: REPORT.open(owner, 1, Surface::Window).unwrap(),
            pending: None,
            sample: 1,
            embedded: false,
        }
    }
    fn event(&mut self, event: &input::Event) -> bool {
        match REPORT.event(self.session, *event) {
            Ok(Some(ActionKind::Refresh)) => {
                self.sample = self.sample.saturating_add(1);
                true
            }
            Ok(Some(ActionKind::Navigate)) => {
                REPORT.close(self.session).unwrap();
                self.embedded = !self.embedded;
                self.session = REPORT
                    .open(
                        self.session.owner,
                        1,
                        if self.embedded {
                            Surface::Region(symbol("report-region"))
                        } else {
                            Surface::Window
                        },
                    )
                    .unwrap();
                true
            }
            Ok(None) => true,
            _ => false,
        }
    }
    fn render(&mut self, output: &mut [u8]) -> Result<usize, Error> {
        let mut value = Text {
            bytes: [0; 64],
            len: 0,
        };
        write!(&mut value, "Odczyt przykładowy: {}", self.sample).map_err(|_| Error::Limit)?;
        let base = REPORT.revision(self.session).map_err(|_| Error::Stale)?;
        let (prepared, len) = REPORT
            .prepare(
                self.session,
                base,
                &contract::CONTRACT,
                TEMPLATE,
                &value,
                self.embedded.then_some(SURROUNDING),
                output,
            )
            .map_err(|_| Error::Binding)?;
        self.pending = Some(prepared);
        Ok(len)
    }
    fn submitted(&mut self, identity: Identity) {
        REPORT
            .commit(self.pending.take().expect("prepared submission"), identity)
            .unwrap();
    }
    fn submission_refused(&mut self) {
        if let Some(token) = self.pending.take() {
            REPORT.discard(token).unwrap();
        }
    }
    fn prepare_shutdown(&mut self) -> bool {
        true
    }
}
impl Drop for Report {
    fn drop(&mut self) {
        REPORT.close(self.session).unwrap();
    }
}
struct Text {
    bytes: [u8; 64],
    len: usize,
}
impl Write for Text {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        self.bytes
            .get_mut(self.len..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}
impl Bindings for Text {
    fn get(&self, key: u32) -> Option<Value<'_>> {
        (key == symbol("sample-value"))
            .then(|| Value::Text(core::str::from_utf8(&self.bytes[..self.len]).unwrap()))
    }
}
#[no_mangle]
pub extern "C" fn _start() -> ! {
    hosted::run::<Report>()
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    osaiku_runtime::process_exit(30)
}
