//! Private transport scratch. Keep it in process storage, not nested control
//! stacks. A mutable borrow owns all bytes through the synchronous trap.
use crate::protocol::channel::{op, Message, BYTES, HEADER};
pub struct Wire {
    bytes: [u8; BYTES],
}
impl Wire {
    pub const EMPTY: Self = Self { bytes: [0; BYTES] };
    pub fn exchange(
        &mut self,
        request: Message,
        payload: &mut [u8],
        invoke: impl FnOnce(&mut [u8; BYTES]) -> isize,
    ) -> Result<Message, isize> {
        self.bytes.fill(0);
        request
            .encode(&mut self.bytes[..HEADER])
            .map_err(|_| -1isize)?;
        if request.operation == op::SUBMIT {
            if request.length as usize != payload.len() || payload.len() > BYTES - HEADER {
                return Err(-1);
            }
            self.bytes[HEADER..HEADER + payload.len()].copy_from_slice(payload);
        }
        let result = invoke(&mut self.bytes);
        if result != 0 {
            return Err(result);
        }
        let reply = Message::decode(&self.bytes[..HEADER]).map_err(|_| -5isize)?;
        if request.operation == op::TAKE {
            let n = usize::try_from(reply.length).map_err(|_| -5isize)?;
            let source = self
                .bytes
                .get(HEADER..HEADER.checked_add(n).ok_or(-5isize)?)
                .ok_or(-5isize)?;
            payload.get_mut(..n).ok_or(-5isize)?.copy_from_slice(source);
        }
        Ok(reply)
    }
}
