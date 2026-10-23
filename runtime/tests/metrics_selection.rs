use osaiku_runtime::{abi::*, metrics};
#[test]
fn selection_is_bounded_and_preserves_transport_refusal() {
    for (op, value) in [(0, 0), (0, 1), (1, 1), (1, u64::MAX)] {
        let mut seen = None;
        assert_eq!(
            metrics::select_with(op, value, |a, b| {
                seen = Some((a, b));
                0
            }),
            Ok(())
        );
        assert_eq!(seen, Some((op, value)));
    }
    for (op, value) in [(0, 2), (1, 0), (1, 2), (2, 0), (u64::MAX, 0)] {
        assert_eq!(
            metrics::select_with(op, value, |_, _| panic!("invalid request crossed boundary")),
            Err(SYSCALL_ERR_INVALID_STATE)
        );
    }
    assert_eq!(
        metrics::select_with(0, 1, |_, _| SYSCALL_ERR_PERMISSION_DENIED),
        Err(SYSCALL_ERR_PERMISSION_DENIED)
    );
}
