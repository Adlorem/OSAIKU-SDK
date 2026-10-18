use osaiku_runtime::{
    application::*,
    process::{ProcessEndpoint, ProcessId},
    window::*,
};

const HOST: ApplicationHost = ApplicationHost::FixedService(ProcessId(1));
const SENDER: ProcessEndpoint = ProcessEndpoint::new(ProcessId(1), 7);

#[test]
fn control_strings_and_exact_matching_preserve_the_existing_protocol() {
    assert_eq!(APPLICATION_COMMAND_REPORT, b"document_app|command|report\n");
    assert_eq!(APPLICATION_COMMAND_EXIT, b"document_app|command|exit\n");
    assert_eq!(APPLICATION_REPLY_RUNNING, b"document_app|reply|running\n");
    assert_eq!(
        APPLICATION_REPLY_INVALID_COMMAND,
        b"document_app|reply|invalid_command\n"
    );
    assert_eq!(
        HOST.classify(SENDER, APPLICATION_COMMAND_REPORT),
        Ok(ApplicationMessage::Report)
    );
    assert_eq!(
        HOST.classify(SENDER, APPLICATION_COMMAND_EXIT),
        Ok(ApplicationMessage::Exit)
    );
    for bytes in [
        b"document_app|command|exit".as_slice(),
        b"document_app|command|exit\nextra",
        APPLICATION_REPLY_RUNNING,
    ] {
        assert_eq!(
            HOST.classify(SENDER, bytes),
            Ok(ApplicationMessage::Other(bytes))
        );
    }
}

#[test]
fn fixed_services_and_reusable_instances_have_distinct_sender_policies() {
    for generation in [0, 1, u32::MAX] {
        let sender = ProcessEndpoint::new(ProcessId(1), generation);
        assert!(HOST.classify(sender, APPLICATION_COMMAND_EXIT).is_ok());
    }
    for sender in [
        ProcessEndpoint::new(ProcessId(2), 7),
        ProcessEndpoint::unbound(ProcessId::KERNEL),
    ] {
        assert_eq!(
            HOST.classify(sender, APPLICATION_COMMAND_EXIT),
            Err(ApplicationMessageError::InvalidSender)
        );
    }
    let instance = ApplicationHost::Instance(ProcessEndpoint::new(ProcessId(8), 3));
    for (sender, accepted) in [
        (ProcessEndpoint::new(ProcessId(8), 3), true),
        (ProcessEndpoint::new(ProcessId(8), 4), false),
        (ProcessEndpoint::new(ProcessId(9), 3), false),
        (ProcessEndpoint::unbound(ProcessId(8)), false),
    ] {
        assert_eq!(
            instance.classify(sender, APPLICATION_COMMAND_EXIT).is_ok(),
            accepted
        );
    }
    for (policy, sender) in [
        (
            ApplicationHost::FixedService(ProcessId::KERNEL),
            ProcessEndpoint::unbound(ProcessId::KERNEL),
        ),
        (
            ApplicationHost::Instance(ProcessEndpoint::unbound(ProcessId(8))),
            ProcessEndpoint::unbound(ProcessId(8)),
        ),
        (
            ApplicationHost::Instance(ProcessEndpoint::new(ProcessId::KERNEL, 3)),
            ProcessEndpoint::new(ProcessId::KERNEL, 3),
        ),
    ] {
        assert_eq!(
            policy.classify(sender, APPLICATION_COMMAND_REPORT),
            Err(ApplicationMessageError::InvalidSender)
        );
    }
}

#[test]
fn envelope_routing_borrows_bytes_without_treating_them_as_valid_metadata() {
    let mut bytes = [0; UI_DOCUMENT_WINDOW_PRESENT_REQUEST_SIZE];
    bytes[..4].copy_from_slice(&UI_DOCUMENT_WINDOW_MAGIC.to_le_bytes());
    bytes[4..8].copy_from_slice(&UI_DOCUMENT_WINDOW_ABI_VERSION.to_le_bytes());
    let ApplicationMessage::Present(borrowed) = HOST.classify(SENDER, &bytes).unwrap() else {
        panic!("expected presentation envelope");
    };
    assert_eq!(borrowed.as_ptr(), bytes.as_ptr());
    assert!(!ui_document_window_present_request_well_formed(
        &UiDocumentWindowPresentRequest::decode(borrowed).unwrap()
    ));
    bytes[4] ^= 1;
    assert_eq!(
        HOST.classify(SENDER, &bytes),
        Ok(ApplicationMessage::Other(&bytes))
    );
    bytes[4] ^= 1;
    bytes[0] ^= 1;
    assert_eq!(
        HOST.classify(SENDER, &bytes),
        Ok(ApplicationMessage::Other(&bytes))
    );
    let arbitrary_notice = [0; 156];
    assert_eq!(
        HOST.classify(SENDER, &arbitrary_notice),
        Ok(ApplicationMessage::Teardown(&arbitrary_notice))
    );
    for size in [1, 155, 157, 159] {
        assert_eq!(
            HOST.classify(SENDER, &bytes[..size]),
            Ok(ApplicationMessage::Other(&bytes[..size]))
        );
    }
}

#[test]
fn invalid_lengths_and_foreign_senders_never_reach_control_routing() {
    let oversized = [0; 161];
    let foreign = ProcessEndpoint::new(ProcessId(2), 7);
    for sender in [SENDER, foreign] {
        for bytes in [&[][..], &oversized[..]] {
            assert_eq!(
                HOST.classify(sender, bytes),
                Err(ApplicationMessageError::InvalidLength)
            );
        }
    }
    for bytes in [
        APPLICATION_COMMAND_EXIT,
        APPLICATION_COMMAND_REPORT,
        &oversized[..156],
        &oversized[..160],
    ] {
        assert_eq!(
            HOST.classify(foreign, bytes),
            Err(ApplicationMessageError::InvalidSender)
        );
    }
}
