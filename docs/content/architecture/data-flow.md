+++
title = "Data Flow"
description = "Connection lifecycle, reactive DataStore, and WebSocket subscriptions"
weight = 2
+++

## Connection Lifecycle

{% mermaid() %}
sequenceDiagram
participant User
participant Controller
participant IntegrationAPI
participant SessionAPI
participant WebSocket
participant DataStore

    User->>Controller: connect()
    Controller->>IntegrationAPI: Authenticate (API key)
    Controller->>SessionAPI: Authenticate (cookie + CSRF)
    Controller->>IntegrationAPI: Fetch all entities
    IntegrationAPI-->>DataStore: Store devices, clients, networks...
    Controller->>SessionAPI: Fetch events, health
    SessionAPI-->>DataStore: Store events, health summaries
    Controller->>WebSocket: Connect event stream
    Controller->>Controller: Spawn refresh task (10s)
    Controller->>Controller: Spawn command processor

    loop Every 10 seconds (TUI default)
        Controller->>IntegrationAPI: Refresh entities
        IntegrationAPI-->>DataStore: Update collections
        DataStore-->>User: Notify via watch channels
    end

    loop Real-time
        WebSocket-->>DataStore: Push events
        DataStore-->>User: Notify subscribers
    end

{% end %}

## Data Store Architecture

The `DataStore` uses a combination of `DashMap` and `tokio::watch` for lock-free reactive storage:

{% mermaid() %}
graph LR
subgraph DataStore
DM["DashMap&lt;String, Arc&lt;T&gt;&gt;<br/><i>Lock-free concurrent map</i>"]
WS["watch::Sender&lt;Arc&lt;Vec&lt;T&gt;&gt;&gt;<br/><i>Change notification</i>"]
end

    API["API Response"] --> DM
    DM --> WS
    WS --> CLI["CLI (read once)"]
    WS --> TUI["TUI (subscribe)"]

{% end %}

- **Writes** go through `DashMap::insert()` then `watch::Sender::send()`
- **CLI reads** call `current()`: snapshot of the latest data
- **TUI subscribes** via `changed()`: async notification on updates

## Entity ID Resolution

Entities can have different IDs depending on the API source:

| API         | ID Format       | Example                                |
| ----------- | --------------- | -------------------------------------- |
| Integration | UUID v4         | `a1b2c3d4-e5f6-7890-abcd-ef1234567890` |
| Session     | ObjectId string | `66a1b2c3d4e5f60718293a4b`             |
| Synthetic   | Prefixed string | `net:a1b2c3d4`, `wifi:e5f6a7b8`        |

The `EntityId` enum handles this transparently:

```rust
enum EntityId {
    Uuid(Uuid),      // Integration API entities
    Legacy(String),  // Session API entities (`_id` ObjectId strings)
}
```

The `Legacy` variant holds the Session API's MongoDB-style `_id` string. Some domain types (clients, devices) key on MAC address as their natural identifier instead, and non-MAC entities (networks, WiFi, firewall policies) use synthetic keys with a type prefix to avoid collisions in the shared `DashMap`.

## CLI vs TUI Data Patterns

| Pattern         | CLI                              | TUI                            |
| --------------- | -------------------------------- | ------------------------------ |
| **Connection**  | `oneshot()`: no background tasks | `connect()`: full lifecycle    |
| **Data access** | Single `current()` snapshot      | `changed()` subscription loop  |
| **Refresh**     | None (fire-and-forget)           | Automatic every 10 seconds     |
| **Events**      | Optional stream command          | Always connected via WebSocket |
