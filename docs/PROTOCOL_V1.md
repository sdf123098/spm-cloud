# SPM Cloud protocol v1

`spm.cloud.v1` is the shared contract between this Rust service and the six SparkleMorpher clients. The authoritative implementation plan and the Java-side P0 contract remain in `D:\SparkleMorpher\docs`; this copy records the backend-owned generated schema entry point.

## Transport

- HTTPS serves typed account, identity, scope, target, appearance, catalog, upload and content APIs.
- WSS `/v1/realtime` carries one Protobuf `Envelope` per binary message.
- Ordinary messages are capped at 64 KiB; file bytes never enter WSS.
- The service never inspects or decrypts SPM model content.

## Identity

The backend stores the UUID separately from its namespace. Wire values are exactly:

```text
official:<uuid>
yggdrasil:<provider_id>:<uuid>
offline:<scope_id>:<uuid>
```

The same UUID in different providers or scopes is not merged. Offline bindings remain scope-local and are returned as `PENDING_APPROVAL` until an explicit approval workflow is added.

## Asset semantics

An asset revision stores the original bytes, SHA-256, byte length and controlled object path. Downloads implement ETag validation and a single byte range with `200`, `206`, `304` and `416` responses. Appearance writes use `expected_revision`; concurrent stale writes return `REVISION_CONFLICT`.

