#![no_std]
#![forbid(unsafe_code)]
//! External build fixture, shipped only with tests, never in an OS image.
use osaiku_driver_protocol::{Error, Header, Kind, Negotiated, Scope, Transfer};
use osaiku_driver_runtime::{Context, Requests};

pub fn receive(scope: Scope, session: Negotiated, message: &[u8]) -> Result<u64, Error> {
    let (header, payload) = Header::decode(session, scope, message)?;
    if header.kind != Kind::Submit {
        return Err(Error::Kind);
    }
    let transfer = Transfer::decode(scope, payload)?;
    let mut requests = Requests::<1>::new(scope)?;
    let request = requests.submit(Context::Service, transfer.range.bytes, 0, transfer.deadline)?;
    requests.complete(scope, request, transfer.range.bytes)?;
    Ok(request.get())
}
