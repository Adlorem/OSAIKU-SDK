# Network service v1 (experimental)

The Stage 33 transport dependency is a user-process IPv4 stack over the Driver
Kit L2 channel. Applications use `osaiku_runtime::network`; the kernel owns the
bounded grant/request ledger, not packet parsing. Linking this API grants no
network access. The initial network configuration is **offline**; the trusted
Wi-Fi boot path may connect using an enrolled installation's saved profiles
and select DHCP after association. Manual configuration and application grants
still require trusted `policyd`; the user consent
workflow and typed outbound request broker are implemented by the native Network
panel and Stage 33 control extension below. Runtime acceptance remains pending.

## Wire and admission

Protocol requirement `NETWORK_SERVICE = 10`, version 1; syscall 408 takes
`[0, writable_buffer, 1152, 0, 0]`. The buffer is copied on entry and return.
`osaiku-network-protocol` defines the shared, allocation-free codec. There are
16 little-endian u64 header words followed by 1024 payload bytes:

| Word | Field |
| --- | --- |
| 0–2 | magic `0x3154454e4941534f`, version 1, operation |
| 3–6 | request id, owner endpoint, grant handle, configuration epoch |
| 7–11 | absolute monotonic deadline in ms, budget, kind, IPv4 address, port |
| 12–15 | flags, payload length, signed result, reserved zero |

IPv4 uses the numeric value of four network-order bytes; upper 32 bits are zero.
No pointer, DMA address or native `sockaddr` crosses this interface. Invalid
length/version/reserved/operation is rejected. Worker threads and document
handlers cannot call it in v1. Use the main process thread and retain its own
request id. Grants bind the complete nonzero process generation and PID.

## Policy operations

`CONFIGURE (1)`: flags 0 offline, 1 static, 2 DHCP; length exactly 16. Static
payload: IPv4 bytes 0–3, prefix length 1–30 at byte 4, zero bytes 5–7, gateway
8–11, DNS server 12–15. Gateway/DNS zero means absent. Static host must not be
network/broadcast; nonzero gateway must be on-link. Offline/DHCP payload is all
zero. Configuration changes advance the epoch and retire all grants/requests.

`GRANT (2)`: exact owner endpoint, kind UDP=1/TCP=2/DNS=3, absolute deadline at
most 60 seconds ahead, positive budget. UDP/TCP bind exactly one unicast IPv4
address and port, with no payload and a maximum 1 MiB application-byte budget.
DNS binds one lowercase ASCII hostname in the payload (1–253 bytes, labels up
to 63), address/port zero, at most 16 queries. The reply supplies a new opaque
handle and epoch. At most eight grants exist globally. Policy selects the
destination; callers cannot retarget a grant. DNS resolution gives **no** UDP
or TCP authority: policy must separately approve the returned address.

`REVOKE (3)`: handle. New operations immediately fail; incomplete requests
become timeout results. The service drops the corresponding socket on its next
turn. Revocation cannot recall packets already handed to the NIC or peer.

## Application operations

Construct `Message::new(operation)`, set the handle and relevant fields; keep
owner/id/result/flags zero. The broker derives destination, owner, kind and
epoch from the grant. A successful submission returns a request id, **not** a
completed operation. Only one unconsumed request per process is allowed.

| Operation | Request / completion |
| --- | --- |
| `OPEN (4)` | Establish outgoing TCP or bind connected UDP; no listeners. |
| `SEND (5)` | 1–1024 payload bytes; completion flags are bytes accepted. TCP may accept a prefix; only send the remainder next. UDP is one datagram. Acceptance is not a remote delivery receipt. |
| `RECEIVE (6)` | Length is capacity 1–1024; completion length/data contain bytes. TCP can return fewer bytes; EOF is `CLOSED`. UDP ignores other peers and rejects an oversized datagram with `LIMIT`. |
| `CLOSE (7)` | TCP FIN completion (closed/time-wait) or immediate UDP/DNS close; completion retires the grant. |
| `RESOLVE (8)` | DNS grant only, request length zero; completion is four IPv4 bytes. A records only. |
| `STATUS (9)` | Flags: 0 closed, 1 connecting, 2 connected, 3 closing. UDP/DNS session reports connected. These values are independent of the stack's internal enum. |
| `CANCEL (10)` | Pending request id; removes the request **and revokes its grant**. Never replay accepted SEND bytes. |
| `POLL (11)` | Request id; `BUSY` until complete, then consumes the result exactly once. Inspect the returned `result`, not only the syscall result. |

Operation deadline zero selects now + 3 seconds; an explicit deadline must be
in the future and no later than the grant deadline. SEND charges requested
length, RECEIVE charges requested capacity, DNS charges one query. Charges are
not refunded on partial completion, timeout or cancellation. This conservative
application budget excludes TCP retransmissions and network control packets.

Errors: INVALID -5, DENIED -8, LIMIT -10, UNSUPPORTED -11, STALE -12, BUSY -13,
TIMEOUT -14, OFFLINE -15, CLOSED -16. A failed submission accepts no request;
BUSY may be retried. After acceptance, poll its id instead of resubmitting.

## Service lifecycle and limits

Only the registered network service endpoint may use TAKE 20, COMPLETE 21,
CONFIGURATION 22, VALIDATE 23 and LINK_STATE 24. Completions must match the
taken id/owner/handle/epoch, fit the request's output bound, and occur once.
The service validates grants each turn; its L2 wait is bounded to 10 ms.
Owner exit clears its grants/requests. Service loss/replacement resets policy
to offline. Link loss advances the epoch and retires grants while preserving
static/DHCP selection; applications need new policy grants after recovery.
DHCP lease loss removes the address and sockets; there is no static fallback.

The profile selects one Ethernet or Wi-Fi interface, eight sessions, 1500-byte
IP MTU, 4096-byte TX/RX socket buffers and fixed 16-frame RX/TX queues. It has
no heap allocator, IPv6, IP fragmentation, listeners, raw sockets, proxy, mDNS,
or packet forwarding. ARP, ICMP echo, DHCP and DNS control traffic is generated
by the service only while enabled. The pinned stack is smoltcp 0.14.0 (0BSD),
with bounded socket storage and checksum validation. It is not linked into
the kernel. Runtime stack/image memory is separately charged to this service.

## Stage 33 / HTTPS boundary

This API supplies transport and consent, not authenticated encryption.
Stage 33A/33C now bind typed outbound intents to exact application instances,
purpose, data class, declared redaction and approved destinations.
TLS 1.3, trusted UTC, certificate-chain and hostname validation, trust-store
lifecycle, bounded HTTPS response streaming and cancellation remain required
before package downloads or browser integration. Plain TCP success is not HTTPS
acceptance. DNS is unauthenticated; validate both policy destination and TLS
identity after resolving. Keep package URL/redirect and timeout restrictions
in the OS installation plan. VM/browser/file/clipboard gates in 33D–33Z remain
separate. None of these depend on Wi-Fi or additional physical NIC families.

## Stage 33 control extension (2026-11-07; acceptance pending)

Existing operation numbers and v1 layouts remain unchanged. New opcodes fail
closed on old kernels; deploy kernel, SDK clients and services together.

| Operation | Contract |
| --- | --- |
| OVERVIEW 30 | Public, no grant. flags desired mode, address medium 0 Ethernet/1 Wi-Fi, epoch configuration generation, kind global state 0 offline/1 no service/2 no link/3 acquiring address/4 configured/5 DHCP timeout. length 34: desired IPv4 record at 0..16, applied at 16..32, medium observations at 32/33 (0 no channel, 1 down, 2 up). result is configuration error. Never socket STATUS or proof of Internet. |
| PUBLISH_STATE 31 | Network service only, matching config epoch. length 18: applied IPv4 and two medium observations; result configuration error. |
| CONTROL 32 | Trusted NetworkManager role only and one-use physical action receipt. kind CONFIGURE=1, WIFI_ON=2, WIFI_OFF=3. CONFIGURE uses existing payload/mode with address selecting medium. Wi-Fi flags 1 permits TPM profiles, length zero. Returns id, not completion. |
| POLICY_TAKE 33 / POLICY_DECIDE 34 | policyd only; exact pending id and owner. Decision revalidates the live application instance. Public callers cannot impersonate the policy service. |
| CONTROL_POLL 35 | Owner/id; BUSY until terminal result, consumed once. Request admission deadline 30 s. Wi-Fi-on completion returns authority epoch and SESSION_DEADLINE (u64::MAX); the panel then asynchronously submits RADIO. Explicit revocation or owner/service termination ends authority; individual operations remain bounded to 30 s. |
| OUTBOUND_REQUEST 36 | Live application only; kernel replaces owner and package identity. Destination, deadline/budget and intent payload described below. Returns stable request id. |
| OUTBOUND_ROW 37 | Panel only; id is queue slot 0..7. Reply id is the stable request; flags 0 pending/1 valid grant/2 refused or revoked. Package name from authenticated application context occupies data 384..512. |
| OUTBOUND_DECIDE 38 | Panel and physical receipt; stable id, flags 1 allow/0 deny. Allow queues policyd validation; it does not directly create a grant. |
| OUTBOUND_RESULT 39 | Request owner/id; BUSY until completed. Inspect result; success supplies the grant handle/epoch for existing OPEN/RESOLVE. Retained until deadline for visible revocation. |
| OUTBOUND_REVOKE 40 | Panel and physical receipt; stable request id. Revokes its actual handle. |

`control::outbound(Destination, purpose, DataClass, Redaction, deadline, budget)`
constructs the typed request. Payload byte 0 is UTF-8 purpose length 1..124,
byte 1 data class (1 metadata, 2 user data), byte 2 redaction (1 not needed,
2 applied), byte 3 zero; purpose at 4..128 with zero padding. TCP/UDP length
128; DNS appends its exact lowercase ASCII name at 128..381. No actual outbound
content or credentials belong in this record. Data class/redaction are caller
declarations, not proof of a transformation. Queue is bounded to eight entries.
Deadline is absolute monotonic milliseconds, no more than 60 s ahead; it includes
time spent awaiting user consent. Budget maximum 1 MiB (TCP/UDP) or 16 DNS queries.
Configuration, link/service loss and process retirement invalidate grants.

`control::configuration` is shared preflight: validates IPv4 fields including
on-link gateway, network/broadcast host rejection, unsupported mode/interface,
and exact payload shape. DHCP acquisition times out after 30 s, clears pending
egress and reports CONFIGURATION_FAILED; Apply or link recovery starts a new
attempt. Up to four L2 channels are retained, but only the first live channel
of the selected medium feeds the single IPv4 stack. Nonselected RX is returned.
The Driver Kit channel receipt now carries medium in byte 75 (0 Ethernet, 1
Wi-Fi), authenticated by the native DirectDevice profile.

### Wi-Fi observations and privacy

Wi-Fi syscall 409 adds OBSERVE 35 (public) and PUBLISH 36 (wifid only). Cached
status expires after 3 s or service replacement. State/capabilities/device/
generation/scan serial remain typed header fields; connected status carries
36 bytes of profile metadata: SSID length, security, two zero bytes, padded
SSID. No credential is returned. Operational STATUS still requires authority.

The panel uses existing asynchronous RADIO, SCAN/SCAN_ROW, profile PUT/GET/
FORGET, CONNECT, CANCEL, DISCONNECT and POLL operations. SSID results have a
30 s TTL and generation/serial checks. Saved profiles use either process/epoch
memory or the existing installation TPM vault. Hiding the panel keeps its process
and radio session alive; terminating a radio owner or its service revokes that
session. Manual Wi-Fi enable selects DHCP first when no Wi-Fi configuration
exists; it preserves an explicit Wi-Fi static/DHCP choice. DHCP acquisition
requires an associated link and is not a prerequisite for starting the radio.

BOOT_SESSION 37 is restricted to the trusted wifid boot client: BEGIN requires
the installation vault, is available once per boot and is refused after manual
radio control. The client starts radio only if it finds a saved profile, scans
for a matching network and makes at most two attempts within 120 s. Following
association, DHCP selection preserves an existing Wi-Fi configuration or a
configured Ethernet with a live link. BOOT_ATTACH 38 lets the authenticated
panel control the service-owned session without changing its epoch or hardware
binding; panel termination detaches that controller without revoking the boot
owner. Manual intervention stops the boot attempt. Unsupported reset/firmware
or TPM admission remains a refusal, never an insecure fallback.

Copied request payloads are scrubbed on drop. The open connection dialog retains
the typed password across submission/errors for correction or retry and scrubs
it on success, cancel, close or session loss. Only a mask enters the declarative
document. Explicit reveal uses a private mutable bitmap
with `gpu_painter::volatile_images()` (bounded draw commands, no texture cache).
The trusted NetworkManager cannot export a window context to an agent.
Acceptance remains scoped to documented tests and individual hardware reports;
it does not establish general NIC compatibility or HTTPS support.
