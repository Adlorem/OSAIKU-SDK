## Redox OS `ps2d`

- Source: https://gitlab.redox-os.org/redox-os/base, `drivers/input/ps2d`
  (`mouse.rs`, `state.rs`, `controller.rs`) at commit
  `fe3a565cd7c7a616099c224e5417d9bdd2a50640`, the last to touch the driver
  being `f416e279`. The GitHub mirror `redox-os/drivers` is archived.
- Used as: the model for the kernel's PS/2 pointing-device driver,
  `sdk/driver-runtime/src/ps2_pointer.rs` (relocated in DRV-09, also used by
  the trusted `kernel/src/drivers/input/ps2_pointer.rs` bootstrap adapter),
  `drivers/ps2-pointer`, and `kernel/src/drivers/input/i8042/auxiliary.rs` -- the order of bring-up
  (reset, self-test, Synaptics identify with four resolution settings and a
  status request, device id, enable), the aux-port command prefix, and the
  packet reassembly on the always-one bit. No source file is copied: Redox's
  driver is a userspace daemon on `std`, and this one is shared `no_std` Rust code
  with its own structure. The Elantech, ALPS and Cypress knocks, the timeouts
  and the zero-magnitude rule are not in `ps2d`; they follow the Synaptics
  TouchPad Interfacing Guide, the protocol notes in Linux's
  `Documentation/input/devices` (`alps.rst`, `elantech.rst`), and the
  observable behaviour of Linux's PS/2 layer. No Linux source is copied.

> MIT License
>
> Copyright (c) 2017 Redox OS
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.
