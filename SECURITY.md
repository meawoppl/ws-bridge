# Security

Report vulnerabilities privately through GitHub security advisories for
[ws-bridge](https://github.com/meawoppl/ws-bridge/security/advisories/new).
The latest 1.x release receives security fixes. Earlier prereleases are unsupported.

Adapters provide typed framing, not authorization. Authenticate and authorize before
upgrading, validate browser Origin headers, and use TLS in production. Configure
message/frame limits, idle deadlines, connection quotas and outbound backpressure in
the underlying framework. Actix aggregation and other framework buffers have their
own limits; consult their configuration APIs. Keep polling receivers for control
frames and use bounded shutdown deadlines. Do not retry non-idempotent operations
without application acknowledgements and deduplication.

Run `cargo audit` in consuming applications: their lockfiles, rather than this
library's lockfile, determine deployed dependency versions. CI audits this repository.
