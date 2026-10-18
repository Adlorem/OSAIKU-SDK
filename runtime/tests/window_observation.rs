//! Own-window metadata validation only: no input, layout or rendering tests.
use osaiku_runtime::{abi::SYSCALL_OK, surface::*};

fn target() -> Target {
    Target {
        owner: (1u64 << 32) | 7,
        window: 8,
        incarnation: 9,
        epoch: 10,
    }
}
fn read(
    mut corrupt: impl FnMut(&mut WindowState, &mut Receipt),
) -> Result<WindowState, PublishError> {
    window_state_with(target(), |request, bytes, receipt| {
        let mut state = WindowState {
            version: 1,
            target: target(),
            visible: 0,
            theme: 0,
            width: 800,
            height: 600,
            geometry_revision: 4,
        };
        receipt.handle = request.handle;
        receipt.epoch = target().epoch;
        corrupt(&mut state, receipt);
        // Every field is a u64; this wire structure contains no padding.
        let encoded = unsafe {
            core::slice::from_raw_parts(
                (&state as *const WindowState).cast::<u8>(),
                core::mem::size_of::<WindowState>(),
            )
        };
        bytes.copy_from_slice(encoded);
        SYSCALL_OK
    })
}
#[test]
fn hidden_window_observation_preserves_owner_identity() {
    let state = read(|_, _| {}).unwrap();
    assert_eq!(state.target, target());
    assert_eq!(state.visible, 0);
    assert_eq!(state.geometry_revision, 4);
}
#[test]
fn stale_foreign_and_malformed_observations_are_rejected() {
    for case in 0..8 {
        assert_eq!(
            read(|state, receipt| match case {
                0 => state.target.owner += 1,
                1 => state.target.epoch += 1,
                2 => receipt.epoch += 1,
                3 => receipt.handle.serial += 1,
                4 => state.visible = 2,
                5 => state.width = 0,
                6 => state.geometry_revision = 0,
                _ => state.version = 2,
            }),
            Err(PublishError::Receipt)
        );
    }
    assert_eq!(
        window_state_with(target(), |_, _, _| -7),
        Err(PublishError::Transport(-7))
    );
}
