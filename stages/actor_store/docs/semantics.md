# Actor Store - Semantic Model

> **Purpose**: This document provides both conceptual and formal semantic models for the minimal in-memory replicated & distributed store with change data capture. It serves as a reasoning and design reference separate from the implementation specification.

---

## Table of Contents

1. [Part I: Conceptual Overview](#part-i-conceptual-overview)
   - [Core Components](#core-components)
   - [Interplay Flows](#interplay-flows)
   - [Key Abstractions](#key-abstractions)
2. [Part II: Formal Semantics](#part-ii-formal-semantics)
   - [Component Signatures](#component-signatures)
   - [Component Interplay](#component-interplay)
   - [State Invariants](#state-invariants)

---

# Part I: Conceptual Overview

> **Focus**: High-level understanding optimized for intuition and system design reasoning.

## Core Components

### 1. Cluster Coordinator

**Responsibilities:**
- Leader election (Raft/Paxos consensus)
- Membership registry (tracks all nodes)
- Health monitoring and failure detection
- Configuration propagation

**Key Characteristics:**
- Uses quorum-based decisions (N/2 + 1)
- Maintains cluster-wide view of node states
- Triggers failover on leader failure
- No data path involvement (control plane only)

---

### 2. In-Memory Store

**Responsibilities:**
- Concurrent hash map storage (e.g., DashMap)
- Handles CRUD operations
- Per-key versioning (monotonic counter)
- Only leader accepts writes

**Key Characteristics:**
- Lock-free concurrent reads
- Version-based optimistic concurrency control
- Integrated with CDC for mutation capture
- No disk dependency (ephemeral state)

---

### 3. Change Log (In-Memory CDC)

**Responsibilities:**
- Circular buffer with sequence numbers
- Captures all mutations (insert/update/delete)
- Retains recent history for replay
- Checkpointed sequence tracking

**Key Characteristics:**
- Fixed-size circular buffer (overwrites oldest)
- Global monotonic sequence numbers
- Supports multiple concurrent subscribers
- Resume capability via sequence numbers
- No WAL/disk persistence (minimal version)

---

### 4. Replication Engine

**Responsibilities:**
- Leader: broadcasts changes to followers
- Follower: applies changes from leader stream
- Async push-based (no waiting for acks)
- Batching for efficiency

**Key Characteristics:**
- Eventual consistency model
- No quorum writes (async replication)
- Tracks follower lag per node
- Auto-reconnects on leader change

---

### 5. Node Sync Manager

**Responsibilities:**
- **Snapshot Transfer**: new node requests full snapshot from leader
- **Change Stream Catch-up**: applies incremental changes from last snapshot sequence
- **Activation Gate**: marks node ready when caught up
- Resume via sequence numbers

**Key Characteristics:**
- Three-phase sync: Snapshot → Stream → Active
- Non-blocking (doesn't block leader)
- Progress tracking for monitoring
- Idempotent change application

---

## Interplay Flows

### Normal Operations

**Client Write Flow:**
```
Client Write Request
  ↓
Leader Store (apply mutation)
  ↓
Change Log (assign sequence number)
  ↓
Replication Engine (async broadcast to followers)
  ↓
Followers (eventually apply)
```

**Client Read Flow:**
```
Client Read Request
  ↓
Any Node Store (eventual consistency)
  OR
Leader Store (strong consistency)
```

---

### Leader Election

**Failure Detection & Election:**
```
Coordinator detects leader failure (heartbeat timeout)
  ↓
Coordinator initiates election (new term)
  ↓
Candidate nodes request votes (quorum N/2 + 1)
  ↓
Quorum reached → New leader elected
  ↓
Followers reconnect to new leader
  ↓
Replication resumes
```

**Key Properties:**
- Safety: At most one leader per term
- Liveness: Eventually elects a leader if majority available
- No data loss: Leader must have latest committed sequence

---

### New Node Join

**Bootstrap & Catch-up:**
```
1. Node contacts Coordinator
   → Receives cluster membership info

2. Node requests snapshot from Leader
   → Receives (data, sequence_number)

3. Node applies snapshot to local store
   → Establishes baseline state

4. Node subscribes to Change Log from sequence + 1
   → Begins streaming incremental changes

5. Node replays changes until caught up
   → Applies changes in sequence order

6. Coordinator marks node as ACTIVE
   → Node participates in reads
```

**Key Properties:**
- Non-blocking: Doesn't interrupt leader operations
- Consistent: Snapshot + changes guarantee no gaps
- Resumable: Can restart from last applied sequence
- Monotonic: Always progressing forward in sequence space

---

### Change Polling with Resume

**Subscriber Pattern:**
```
Subscriber tracks last_seen_sequence = N
  ↓
Poll Change Log from sequence N + 1
  ↓
Receive batch of changes [N+1, N+2, ..., N+M]
  ↓
Process changes
  ↓
Update last_seen_sequence = N + M
  ↓
Repeat (with backoff if no new changes)
```

**Key Properties:**
- Pull-based (consumer controls rate)
- Batching for efficiency
- Idempotent (can re-request same sequence)
- No message loss (within buffer retention)

---

## Key Abstractions

### Sequence Numbers
- **Definition**: Global, monotonic ordering across all changes
- **Scope**: Cluster-wide (not per-key)
- **Properties**: Strictly increasing, never reused, gap-free
- **Use Cases**: Resume tokens, ordering guarantees, replication checkpoints

### Node Roles
- **LEADER**: Accepts writes, broadcasts to followers, serves consistent reads
- **FOLLOWER**: Replicates from leader, serves eventually-consistent reads
- **JOINING**: Syncing state (snapshot + catch-up), not serving reads yet

### Snapshot
- **Definition**: Point-in-time copy of entire store + sequence marker
- **Contents**: All key-value pairs + versions + as_of_sequence
- **Properties**: Immutable, consistent (single sequence point)
- **Use Cases**: New node bootstrap, backup/restore, debugging

### Change Stream
- **Definition**: Ordered log of mutations with resume capability
- **Structure**: (sequence, operation, key, prev_value, new_value, timestamp)
- **Properties**: Ordered, replayable, filterable
- **Use Cases**: CDC subscribers, replication, audit trails

### Quorum
- **Definition**: N/2 + 1 nodes
- **Scope**: Leader election only (not for writes)
- **Properties**: Ensures single leader per term, survives minority failures
- **Trade-off**: Async replication for availability over strict consistency

---

# Part II: Formal Semantics

> **Focus**: Formal state machine definitions optimized for rigorous reasoning and verification.

## Component Signatures

### 1. In-Memory Store (with integrated CDC)

**State:**
```
StoreState = {
  data: Map<Key, (Value, Version)>,
  cdc_writer: CdcWriter
}
```

**Commands:**
```
Command =
  | Insert(key: Key, value: Value)
  | Get(key: Key)
  | Update(key: Key, value: Value)
  | CAS(key: Key, value: Value, expected_version: Version)
  | Delete(key: Key)
  | GetAll()
```

**State Transition:**
```
(StoreState, Command) → (StoreState', Result, Option<ChangeEvent>)

where:
  Result =
    | Ok(ActorData)
    | Error(NotFound | VersionMismatch | AlreadyExists)

  ChangeEvent = {
    sequence: u64,
    operation: Insert | Update | Delete,
    key: Key,
    prev_value: Option<Value>,
    new_value: Option<Value>,
    timestamp: DateTime
  }
```

**Semantics:**
- `Insert`: Creates new entry with version 0, fails if key exists, produces INSERT event
- `Get`: Returns current value, no state change, no event
- `Update`: Increments version, replaces value, produces UPDATE event with previous value
- `CAS`: Updates only if version matches, produces UPDATE event or VersionMismatch error
- `Delete`: Removes entry, produces DELETE event with previous value

---

### 2. Change Log (In-Memory CDC)

**State:**
```
LogState = {
  buffer: CircularBuffer<ChangeEvent>,       // fixed size, overwrites oldest
  next_sequence: u64,                        // next to assign
  subscribers: Set<SubscriberId>,
  checkpoint: u64                            // earliest available sequence in buffer
}
```

**Commands:**
```
Command =
  | Append(unsequenced_event: UnsequencedEvent)
  | Subscribe(from_position: StartPosition)
  | Poll(subscriber_id: SubscriberId, last_seen: u64, batch_size: usize)
  | Unsubscribe(subscriber_id: SubscriberId)
  | GetCheckpoint()

where:
  StartPosition = Beginning | Sequence(u64) | Now
```

**State Transition:**
```
(LogState, Command) → (LogState', Result, Broadcast)

where:
  Result =
    | Sequence(u64)                   // Append result
    | Subscription(SubscriberId)      // Subscribe result
    | Vec<ChangeEvent>                // Poll result
    | Checkpoint(u64)                 // GetCheckpoint result

  Broadcast = Option<ChangeEvent>     // to all active subscribers
```

**Semantics:**
- `Append`: Assigns next_sequence, adds to buffer, broadcasts to subscribers
- `Subscribe(Beginning)`: Starts from checkpoint (earliest available)
- `Subscribe(Sequence(N))`: Starts from N if available, else returns error
- `Subscribe(Now)`: Starts from next_sequence (only new events)
- `Poll`: Returns up to batch_size events after last_seen, empty if caught up
- `GetCheckpoint`: Returns earliest sequence still in buffer

**Invariants:**
```
1. next_sequence is strictly monotonic
2. checkpoint ≤ next_sequence ≤ checkpoint + buffer_size
3. ∀ seq ∈ buffer: checkpoint ≤ seq < next_sequence
4. buffer is a sliding window: oldest events evicted when full
```

---

### 3. Replication Engine

**State:**
```
ReplState = {
  role: Leader | Follower,
  leader_endpoint: Option<Address>,
  follower_positions: Map<NodeId, u64>,     // last ack'd sequence per follower
  pending_queue: Queue<ChangeEvent>
}
```

**Commands:**
```
Command =
  | Push(change_event: ChangeEvent)              // leader: push to followers
  | Pull(from_sequence: u64)                     // follower: pull from leader
  | Ack(node_id: NodeId, sequence: u64)          // follower acks receipt
  | PromoteToLeader()
  | DemoteToFollower(leader_addr: Address)
```

**State Transition:**
```
(ReplState, Command) → (ReplState', Result, Multicast<NodeId, ChangeEvent>)

where:
  Result =
    | Ok
    | Batch<ChangeEvent>
    | LagInfo(node_id: NodeId, lag_count: u64)

  Multicast = Vec<(NodeId, ChangeEvent)>        // async sends to followers
```

**Semantics:**
- `Push`: Leader enqueues event, sends to all followers async
- `Pull`: Follower requests events from sequence N, receives batch
- `Ack`: Follower confirms receipt, leader updates follower_positions
- `PromoteToLeader`: Transition to leader role, clear follower_positions
- `DemoteToFollower`: Transition to follower, set leader_endpoint, flush pending_queue

**Invariants:**
```
1. ∀ follower ∈ follower_positions: follower.sequence ≤ leader.next_sequence
2. role = Leader ⟹ leader_endpoint = None
3. role = Follower ⟹ leader_endpoint = Some(addr)
4. pending_queue contains only events not yet ack'd by all followers
```

---

### 4. Cluster Coordinator

**State:**
```
CoordState = {
  membership: Map<NodeId, NodeInfo>,
  term: u64,                                    // current election term
  leader: Option<NodeId>,
  node_health: Map<NodeId, (LastHeartbeat, Status)>,
  pending_votes: Map<Term, Set<NodeId>>
}

where:
  NodeInfo = { address: Address, role: NodeRole, joined_at: DateTime }
  Status = Healthy | Suspected | Failed
  NodeRole = Leader | Follower | Joining
```

**Commands:**
```
Command =
  | Join(node_id: NodeId, address: Address)
  | Leave(node_id: NodeId)
  | Heartbeat(node_id: NodeId, timestamp: DateTime)
  | RequestVote(candidate_id: NodeId, term: u64)
  | GrantVote(voter_id: NodeId, term: u64, candidate_id: NodeId)
  | DeclareLeader(node_id: NodeId, term: u64)
  | GetClusterView()
```

**State Transition:**
```
(CoordState, Command) → (CoordState', Result, Event)

where:
  Result =
    | ClusterView(Vec<NodeInfo>)
    | VoteGranted(bool)
    | LeaderInfo(node_id: NodeId, term: u64)

  Event =
    | MembershipChange(added: Option<NodeId>, removed: Option<NodeId>)
    | LeaderElected(node_id: NodeId, term: u64)
    | NodeFailed(node_id: NodeId)
```

**Semantics:**
- `Join`: Adds node to membership, initializes health tracking, emits MembershipChange
- `Leave`: Removes node, triggers re-election if was leader
- `Heartbeat`: Updates node_health timestamp, marks as Healthy
- `RequestVote`: Records vote request, checks term validity
- `GrantVote`: Increments vote count for candidate, grants if term ≥ current_term
- `DeclareLeader`: Sets leader when quorum reached, increments term, emits LeaderElected

**Invariants:**
```
1. ∀ term: |{n | pending_votes[term] contains n}| ≤ |membership|
2. leader = Some(n) ⟹ n ∈ membership
3. ∀ term₁, term₂: term₁ < term₂ ⟹ leader(term₂) elected after leader(term₁)
4. Quorum = |membership| / 2 + 1
5. ∀ node ∈ membership: ∃ health_entry ∈ node_health
```

---

### 5. Node Sync Manager

**State:**
```
SyncState = {
  phase: SyncPhase,
  snapshot_sequence: Option<u64>,
  applied_until: u64,
  target_sequence: Option<u64>
}

where:
  SyncPhase =
    | Idle
    | RequestingSnapshot
    | ApplyingSnapshot
    | StreamingChanges
    | Active
```

**Commands:**
```
Command =
  | StartSync(leader_address: Address)
  | ReceiveSnapshot(data: Map<Key, (Value, Version)>, as_of_sequence: u64)
  | ApplyChange(change_event: ChangeEvent)
  | CheckIfCaughtUp()
  | MarkActive()
```

**State Transition:**
```
(SyncState, Command) → (SyncState', Result, PhaseTransition)

where:
  Result =
    | Progress(applied: u64, remaining: Option<u64>)
    | Ready
    | StillCatchingUp

  PhaseTransition =
    | None
    | ToPhase(new_phase: SyncPhase)
```

**Semantics:**
- `StartSync`: Transitions Idle → RequestingSnapshot, initiates snapshot request
- `ReceiveSnapshot`: Stores snapshot_sequence, transitions to ApplyingSnapshot
- `ApplyChange`: Increments applied_until, applies change to local store
- `CheckIfCaughtUp`: Compares applied_until with target_sequence
- `MarkActive`: Transitions to Active phase

**Phase Transitions:**
```
Idle
  --[StartSync]--> RequestingSnapshot
  --[ReceiveSnapshot]--> ApplyingSnapshot
  --[snapshot applied]--> StreamingChanges
  --[CheckIfCaughtUp && caught_up]--> Active

Active is terminal (unless node restarts)
```

**Invariants:**
```
1. phase = ApplyingSnapshot ⟹ snapshot_sequence = Some(seq)
2. phase = StreamingChanges ⟹ applied_until ≥ snapshot_sequence.unwrap()
3. phase = Active ⟹ applied_until ≈ leader.next_sequence (within buffer lag)
4. applied_until is monotonically increasing
```

---

## Component Interplay

### Write Path (Normal Operation)

```
Client: Command(Insert(k, v))
  ↓
Store: (S, Insert(k,v)) → (S', Ok(data@v0), ChangeEvent₁)
  ↓
ChangeLog: (LS, Append(ChangeEvent₁)) → (LS', Seq(100), Broadcast(ChangeEvent₁@100))
  ↓
ReplicationEngine: (RS, Push(ChangeEvent₁@100)) → (RS', Ok, Multicast([follower₁, follower₂], ChangeEvent₁@100))
  ↓
Followers: Async receive and apply
```

**Composition Properties:**
- Sequential: Store → ChangeLog → Replication
- Synchronous: Client waits only for Store + ChangeLog
- Asynchronous: Replication happens in background
- Linearizable: Client observes immediate effect on leader

---

### Read Path

**Eventual Consistency (any node):**
```
Client: Command(Get(k))
  ↓
Store: (S, Get(k)) → (S, Ok(data@vN), ∅)
```

**Strong Consistency (leader only):**
```
Client: Command(Get(k)) [with consistency_level=Strong]
  ↓
Coordinator: Verify current leader
  ↓
Store(leader): (S, Get(k)) → (S, Ok(data@vN), ∅)
```

---

### Leader Election Flow

```
Coordinator: Heartbeat timeout detected for leader
  ↓
Coordinator: (CS, RequestVote(node₂, term=5))
           → (CS', Vote requests multicast, ∅)
  ↓
Coordinator: (CS', GrantVote(node₃, term=5, node₂))
           → (CS'', VoteGranted(true), ∅)
  ... [collect votes from quorum]
  ↓
Coordinator: (CS^n, DeclareLeader(node₂, term=5))
           → (CS^(n+1), LeaderInfo(node₂, 5), Event(LeaderElected))
  ↓
ReplicationEngine: (RS, PromoteToLeader())
                 → (RS', Ok, ∅)
```

**Safety Property:**
```
∀ term: |{n | DeclareLeader(n, term) was executed}| ≤ 1
```

---

### New Node Join & Sync Flow

```
1. Coordinator: (CS, Join(node₄, addr))
              → (CS', ClusterView, Event(MemberAdded))

2. SyncManager: (Idle, StartSync(leader_addr))
              → (RequestingSnapshot, Progress(0, None), ToPhase(RequestingSnapshot))

3. Leader.Store: (S, GetAll())
               → (S, Snapshot(data, seq=5000), ∅)

4. SyncManager: (RequestingSnapshot, ReceiveSnapshot(data, 5000))
              → (ApplyingSnapshot, Progress, ToPhase(ApplyingSnapshot))

5. Store(node₄): Apply snapshot → local state populated

6. SyncManager: (ApplyingSnapshot, CheckIfCaughtUp())
              → (StreamingChanges, StillCatchingUp, ToPhase(StreamingChanges))

7. ChangeLog: (LS, Subscribe(Sequence(5001)))
            → (LS', Subscription(sub₄), ∅)

8. Loop:
   ChangeLog: (LS, Poll(sub₄, 5001, 100))
            → (LS, Vec<ChangeEvent[5001..5100]>, ∅)

   SyncManager: (StreamingChanges, ApplyChange(event₅₀₀₁))
              → (StreamingChanges', Progress(5001, Some(5499)), None)

   ... repeat for events 5002..5500

9. SyncManager: (StreamingChanges, CheckIfCaughtUp())
              → (Active, Ready, ToPhase(Active))

10. Coordinator: (CS, Heartbeat(node₄, now))
               → (CS', Ok, ∅)  [node now healthy & active]
```

**Progress Invariant:**
```
∀ step i: SyncManager.applied_until[i] ≥ SyncManager.applied_until[i-1]
```

---

### CDC Polling with Resume Flow

```
Client: Subscribe(Beginning)
  ↓
ChangeLog: (LS, Subscribe(Beginning))
         → (LS', Subscription(id=42), ∅)

Loop (N = 0 initially):
  Client: Poll(42, last_seen=N, batch=50)
    ↓
  ChangeLog: (LS, Poll(42, N, 50))
           → (LS, Vec<ChangeEvent[N+1..N+50]>, ∅)
    ↓
  Client: Process events [N+1..N+50]
    ↓
  Client: Update last_seen := N+50
    ↓
  Client: Poll(42, last_seen=N+50, batch=50)
    ↓
  ... repeat
```

**Resume Property:**
```
If client crashes and restarts with last_seen=K:
  ∧ K ≥ ChangeLog.checkpoint
  ⟹ Client can resume from sequence K+1 without data loss

If K < ChangeLog.checkpoint:
  ⟹ Client must re-subscribe from Beginning (gap occurred)
```

---

## State Invariants

### Global Invariants

```
I1. Node Membership Consistency:
    ∀ node ∈ Coordinator.membership:
      node is reachable ∧ node.health ≠ Failed

I2. Single Leader Property:
    ∃! leader_node:
      Coordinator.leader = Some(leader_node)
      ∧ ReplicationEngine(leader_node).role = Leader

I3. Sequence Monotonicity:
    ∀ event₁, event₂ ∈ ChangeLog.buffer:
      event₁.sequence < event₂.sequence ⟹ event₁ occurred before event₂

I4. Eventual Consistency:
    ∀ follower ∈ Followers:
      Eventually: Store(follower).data ⊇ Store(leader).data[0..checkpoint]
      (modulo events still in replication pipeline)

I5. Active Node Liveness:
    ∀ node where SyncManager.phase = Active:
      node.applied_until ≈ ChangeLog.next_sequence
      (within buffer_size lag tolerance)

I6. Circular Buffer Bounds:
    ChangeLog.checkpoint ≤ ChangeLog.next_sequence ≤ ChangeLog.checkpoint + buffer_size

I7. Version Monotonicity Per Key:
    ∀ key ∈ Store.data:
      ∀ events e₁, e₂ affecting key:
        e₁.sequence < e₂.sequence ⟹ e₁.version < e₂.version

I8. Leader Term Monotonicity:
    ∀ term₁, term₂ where term₁ < term₂:
      leader(term₂) was elected after leader(term₁) failed or stepped down
```

### Component-Local Invariants

**Store:**
```
S1. ∀ key ∈ Store.data: key is non-empty
S2. ∀ (key, (value, version)) ∈ Store.data: version ≥ 0
S3. Insert(k, v) succeeds ⟹ k ∉ Store.data before operation
S4. Update(k, v) succeeds ⟹ k ∈ Store.data before operation
S5. CAS(k, v, expected_v) succeeds ⟹ Store.data[k].version = expected_v
```

**ChangeLog:**
```
L1. next_sequence is strictly increasing
L2. ∀ event ∈ buffer: checkpoint ≤ event.sequence < next_sequence
L3. buffer is sorted by sequence (oldest to newest)
L4. |buffer| ≤ buffer_size
L5. ∀ subscriber: subscriber.last_seen ≥ checkpoint ⟹ can resume without data loss
```

**Replication Engine:**
```
R1. role = Leader ⟹ follower_positions.len() = |cluster| - 1
R2. role = Follower ⟹ leader_endpoint = Some(valid_address)
R3. ∀ follower: follower_positions[follower] ≤ leader.next_sequence
R4. pending_queue contains events not yet ack'd by all followers
```

**Coordinator:**
```
C1. ∃ leader ⟹ leader received quorum votes in current term
C2. ∀ node: node.last_heartbeat < timeout ⟹ node.status = Healthy
C3. ∀ term: ∃ at most one leader
C4. membership changes are linearizable (total order)
```

**Sync Manager:**
```
M1. phase = Idle ⟹ snapshot_sequence = None ∧ applied_until = 0
M2. phase = ApplyingSnapshot ⟹ snapshot_sequence = Some(seq)
M3. phase = StreamingChanges ⟹ applied_until ≥ snapshot_sequence.unwrap()
M4. phase = Active ⟹ node participates in reads
M5. applied_until never decreases
```

---

## Summary

This semantic model provides:

1. **Conceptual Clarity**: Part I offers intuitive understanding of components and their interactions
2. **Formal Rigor**: Part II enables precise reasoning about state transitions and correctness
3. **Consistency**: Both parts describe the same system from complementary perspectives
4. **Verifiability**: Invariants provide checkable properties for testing and verification

The model explicitly excludes:
- Disk persistence (WAL, snapshots to disk)
- Authentication/authorization
- Network protocol details
- Performance optimizations (batching specifics, compression, etc.)

This minimal design focuses on core distributed consensus, replication, and change data capture semantics.