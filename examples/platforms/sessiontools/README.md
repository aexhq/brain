# Platform-owned session inspection

This example belongs to the application layer. Copy it into the platform that owns session discovery, target authorization and product policy. Brain's `integrations` package supplies HTTP/MCP endpoint constructors; its public `Server.ReadSession` and `Server.ReadEvents` methods enforce tenant scope and return public data.

`Sessions(reader, authorize)` shows how a platform can bind a model-visible `read` tool. The authenticated binding supplies the tenant, and the platform authorizes every target independently. The operation returns a bounded page of public events and metadata, excludes native/private credential state and never starts inference. Its tests retain cross-tenant and target-denial checks after the provider's relocation.

Keep one owning Brain instance for every session. Eventual metadata discovery is not an ownership lease. Route all session operations to that owner and fence it before failover. Your platform also owns retention and deletion: stop admission/execution, revoke grants and external callbacks, stop checkpoint publication, and remove retained local, index, archive and tool-service copies under the selected policy. Session closure releases execution; it is not a physical-delete acknowledgement.
