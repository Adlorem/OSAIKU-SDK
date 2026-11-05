# Network service v1 (experimental)

The Stage 33 transport dependency is a user-process IPv4 stack over the Driver
Kit L2 channel. Applications use `osaiku_runtime::network`; the kernel owns the
bounded grant/request ledger, not packet parsing. Linking this API grants no
network access. The production boot policy is **offline**. Only trusted
`policyd` can configure the interface and issue/revoke grants; the user consent
workflow and typed outbound request broker are subsequent Stage 33 work.

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

The initial profile supports one Ethernet interface, eight sessions, 1500-byte
IP MTU, 4096-byte TX/RX socket buffers and fixed 16-frame RX/TX queues. It has
no heap allocator, IPv6, IP fragmentation, listeners, raw sockets, proxy, mDNS,
or packet forwarding. ARP, ICMP echo, DHCP and DNS control traffic is generated
by the service only while enabled. The pinned stack is smoltcp 0.14.0 (0BSD),
with bounded socket storage and checksum validation. It is not linked into
the kernel. Runtime stack/image memory is separately charged to this service.

## Stage 33 / HTTPS boundary

This API supplies transport, not authenticated encryption or permission UI.
Stage 33A still supplies user consent/policy provisioning; 33C must bind typed
outbound requests to purpose, data class, redaction and approved destinations.
TLS 1.3, trusted UTC, certificate-chain and hostname validation, trust-store
lifecycle, bounded HTTPS response streaming and cancellation remain required
before package downloads or browser integration. Plain TCP success is not HTTPS
acceptance. DNS is unauthenticated; validate both policy destination and TLS
identity after resolving. Keep package URL/redirect and timeout restrictions
in the OS installation plan. VM/browser/file/clipboard gates in 33D–33Z remain
separate. None of these depend on Wi-Fi or additional physical NIC families.
