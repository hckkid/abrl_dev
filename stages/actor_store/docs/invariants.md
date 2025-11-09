# Distributed Systems Invariants

This document catalogs the distributed systems properties and invariants for the Actor Store implementation. It serves as a checklist for correctness, reliability, and consistency guarantees in the distributed environment.

## Overview

Distributed systems face unique challenges including network partitions, concurrent operations, node failures, and timing uncertainties. This document tracks:

- ✅ **Implemented Properties**: Invariants currently enforced with tests
- ⚠️ **Recommended Properties**: Important invariants not yet implemented

> **Note**: Implementation references in this document point to the `actor_store_minimal` stage located at `../actor_store_minimal/`. This stage contains the full distributed implementation with replication, failover, and CDC consistency features.

---

## Already Implemented Properties

### 1. Split-Brain Write Protection ✅

**Property**: Writes from a deposed leader must be discarded when it rejoins the cluster.

**Definition**:
When a leader is partitioned from the cluster and a new leader is elected (new term), any writes accepted by the old leader during the partition must be rolled back when it rejoins. Writes are only committed if acknowledged by the current term's leader.

**Example Scenario**:
```
t0: Node1 is leader (term=1)
t1: Network partition: Node1 isolated from Node2, Node3
t2: Node2 becomes leader (term=2) via election
t3: Node1 still thinks it's leader, accepts write W1 (stale, term=1)
t4: Node2 accepts write W2 (valid, term=2)
t5: Partition heals
t6: Node1 rejoins, must rollback W1 (term=1 < term=2)
```

**Implementation**:
- `src/store/store.rs:32-38` - Term tracking (`current_term`, `set_term`, `get_term`)
- `src/store/store.rs:87-96` - Rollback mechanism (`rollback_uncommitted_writes`)
- `src/store/cdc.rs:15-25` - ChangeEvent includes `term` field
- `src/store/failover.rs:68-96` - `handle_follower_reconnect` detects term mismatch and triggers rollback

**Tests**:
- `src/main.rs` - Test D1: Split-brain write discarded

---

### 2. CDC Read Consistency ✅

**Property**: Change Data Capture subscribers only observe events that have been durably replicated according to the configured consistency level.

**Definition**:
CDC events are only visible to subscribers once they've been acknowledged by:
- **Majority**: N/2 + 1 replicas (including leader)
- **All**: All N replicas

This prevents exposing uncommitted events that might be rolled back during failures.

**Example Scenario (Majority)**:
```
Cluster: 3 nodes (Leader, F1, F2)
Consistency: Majority (need 2 acks)

t0: Leader writes event E1 (seq=1), locally applied
t1: CDC poll → returns [] (only 1 ack: leader itself)
t2: F1 acknowledges seq=1
t3: CDC poll → returns [E1] (2 acks: quorum reached)
```

**Example Scenario (All)**:
```
Cluster: 3 nodes (Leader, F1, F2)
Consistency: All (need all followers)

t0: Leader writes E1 (seq=1)
t1: F1 acknowledges seq=1
t2: CDC poll → returns [] (only 1 follower ack, need 2)
t3: F2 acknowledges seq=1
t4: CDC poll → returns [E1] (all followers acked)
```

**Implementation**:
- `src/store/cdc.rs:29-33` - `CdcConsistency` enum (Majority/All)
- `src/store/cdc.rs:53-55` - `committed_sequence` watermark tracking
- `src/store/cdc.rs:137-163` - `poll()` filters by committed sequence
- `src/store/cdc.rs:171-202` - `update_committed_sequence()` calculates quorum
- `src/store/replication.rs:22` - `Ack` message includes `up_to_sequence`

**Tests**:
- `src/main.rs` - Test D2: CDC Majority consistency
- `src/main.rs` - Test D3: CDC All-nodes consistency

---

### 3. Sequence Monotonicity ✅

**Property**: Change event sequence numbers are strictly monotonically increasing and unique.

**Definition**:
Each change event is assigned a unique, monotonically increasing sequence number. No two events share the same sequence, and sequences never decrease. This provides a total order for all changes.

**Example Scenario**:
```
t0: Insert actor A → event seq=1
t1: Update actor A → event seq=2
t2: Insert actor B → event seq=3
t3: Delete actor A → event seq=4

Invariant: seq(t0) < seq(t1) < seq(t2) < seq(t3)
```

**Implementation**:
- `src/store/cdc.rs:49` - `next_sequence: AtomicU64` for atomic sequence generation
- `src/store/cdc.rs:76-87` - `append()` uses `fetch_add` for atomic increment
- `src/store/cdc.rs:94-96` - `next_sequence()` exposes current sequence

**Tests**:
- All CDC tests implicitly verify monotonicity by checking sequence ordering

---

### 4. Circular Buffer Bounds ✅

**Property**: The change log maintains a bounded circular buffer, automatically evicting old events when capacity is reached.

**Definition**:
The change log buffer has a fixed maximum size. When full, appending new events evicts the oldest event (FIFO). Subscribers attempting to read evicted sequences receive an error. This prevents unbounded memory growth.

**Example Scenario**:
```
Buffer capacity: 3 events

t0: Append E1 (seq=1) → buffer=[E1]
t1: Append E2 (seq=2) → buffer=[E1, E2]
t2: Append E3 (seq=3) → buffer=[E1, E2, E3]
t3: Append E4 (seq=4) → buffer=[E2, E3, E4]  // E1 evicted
t4: Subscriber tries to read seq=1 → Error: "before checkpoint"
```

**Implementation**:
- `src/store/cdc.rs:48-50` - `buffer: VecDeque<ChangeEvent>` with fixed `buffer_size`
- `src/store/cdc.rs:80-84` - `append()` pops front when buffer is full
- `src/store/cdc.rs:89-92` - `get_checkpoint()` returns oldest available sequence
- `src/store/cdc.rs:112-117` - `subscribe()` validates sequence is after checkpoint

**Tests**:
- Implicit in CDC tests with buffer size limits

---

### 5. Single Leader per Term ✅

**Property**: At most one leader exists for any given term.

**Definition**:
The election protocol ensures that in each term, at most one node can become leader. A node can only win election by receiving votes from a majority of nodes, and each node votes for at most one candidate per term. This prevents split-brain at the election level.

**Example Scenario**:
```
Cluster: 3 nodes (N1, N2, N3)
Term 5:

N1 requests votes for term 5
N2 votes for N1 in term 5
N3 votes for N1 in term 5
→ N1 becomes leader (2 votes, quorum=2)

Later, N2 requests votes for term 5
N3 says: "Already voted for N1 in term 5"
→ N2 cannot become leader in same term

Invariant: ∀ term, |{leaders in term}| ≤ 1
```

**Implementation**:
- `src/store/election.rs:24-26` - `voted_for` tracks single vote per term
- `src/store/election.rs:49-81` - `handle_vote_request()` enforces one vote per term
- `src/store/election.rs:57-62` - New term resets `voted_for`
- `src/store/election.rs:62-70` - Same term checks if already voted

**Tests**:
- `src/main.rs` - Integration tests verify election produces single leader

---

### 6. Quorum-based Election ✅

**Property**: A node can only become leader by obtaining votes from a majority (quorum) of nodes.

**Definition**:
Election success requires N/2 + 1 votes (including self-vote). This ensures that any two majorities overlap, preventing conflicting leaders. The quorum property is fundamental to maintaining consistency across network partitions.

**Example Scenario**:
```
Cluster: 5 nodes
Quorum: 5/2 + 1 = 3 votes

Scenario A - Success:
N1 starts election, receives votes from: N1, N2, N3
→ 3 votes ≥ quorum(3) → N1 becomes leader ✓

Scenario B - Failure:
N1 partitioned from N3, N4, N5
N1 receives votes from: N1, N2
→ 2 votes < quorum(3) → N1 cannot become leader ✗
```

**Implementation**:
- `src/store/election.rs:100-108` - `check_quorum()` validates N/2 + 1
- `src/store/election.rs:103` - Quorum calculation: `(cluster_size / 2) + 1`
- `src/store/election.rs:30-47` - `start_election()` initializes with self-vote

**Tests**:
- `src/store/failover.rs:49` - Uses `check_quorum()` before promoting to leader

---

## Recommended Properties (Not Yet Implemented)

### 7. Zombie Leader Protection ⚠️

**Property**: A leader that has been deposed but doesn't know it yet (zombie leader) cannot accept writes.

**Definition**:
The current implementation only checks local role state. Between the time a leader is deposed (e.g., network partition) and when it discovers this (via term mismatch), it can still accept writes. These writes will eventually be rolled back, but clients receive false acknowledgments. Fencing tokens (generation IDs) would prevent zombies from accepting writes in the first place.

**Example Scenario**:
```
t0: Node1 is leader (term=5)
t1: Network partition: Node1 isolated
t2: Node2 elected leader (term=6)
t3: Client C1 sends write W1 to Node1
t4: Node1 thinks it's still leader, validates:
    - is_current_leader() → true ✓ (stale local state)
    - Accepts W1, returns success to C1
t5: Client C1 thinks W1 is committed
t6: Partition heals, Node1 rolls back W1
t7: Client C1's write is lost, but it doesn't know!

Fix: Fencing tokens - Node2 (term=6) gets token T6
      Storage layer rejects operations with token < T6
      Node1 tries write with token T5 → rejected immediately
```

**Recommendation**:
Implement generation IDs / fencing tokens at the storage layer. Each leadership change increments a global generation number that must be included in all write operations.

---

### 8. Idempotency ⚠️

**Property**: Duplicate requests (e.g., from network retries) should be detected and deduplicated.

**Definition**:
Networks can deliver the same message multiple times. Without idempotency, retries cause duplicate operations and duplicate CDC events. Each request should include a unique `request_id`, and the system should track recently processed requests to detect duplicates.

**Example Scenario**:
```
Client sends: Create(actor=Alice, request_id=req-123)

t0: Request arrives, processed → seq=1, returns success
t1: Network issue, client doesn't receive response
t2: Client retries: Create(actor=Alice, request_id=req-123)
t3: Without dedup → creates duplicate event seq=2 (wrong!)
    With dedup → detects req-123 already processed, returns cached result

CDC subscriber sees:
Without dedup: [Create(Alice, seq=1), Create(Alice, seq=2)]  ← wrong!
With dedup:    [Create(Alice, seq=1)]                         ← correct
```

**Recommendation**:
- Add `request_id` field to all commands
- Maintain a cache of recently processed `(request_id, response)` pairs
- Check cache before executing operations

---

### 9. Linearizable Reads ⚠️

**Property**: Reads should return the most recent committed value, not arbitrarily stale data.

**Definition**:
Currently, followers can serve reads from their local state without checking if they're partitioned from the leader. This violates linearizability - reads can return data from the distant past. Need read consistency levels:
- **Linearizable**: Read from leader or check leader health before follower read
- **BoundedStaleness**: Read from follower with max staleness bound (e.g., 5 seconds)
- **Eventual**: Read from any replica (current behavior)

**Example Scenario**:
```
t0: Leader writes: actor A = {version: 5}
t1: Write replicates to F1, F2
t2: Network partition isolates F1
t3: Leader writes: actor A = {version: 6}
t4: Client reads from F1 → gets {version: 5} (stale!)
t5: Client reads from Leader → gets {version: 6}

Client sees version go backward: 6 → 5
Violates linearizability
```

**Recommendation**:
- Implement read consistency levels
- Leader reads: Always linearizable
- Follower reads: Include leader lease check or bounded staleness check

---

### 10. Lost Update Prevention ⚠️

**Property**: Concurrent updates should not silently overwrite each other.

**Definition**:
The `ActorData` type includes a `version` field and `CollectionCommand` supports CAS (Compare-And-Swap), but it's not enforced. Two concurrent updates can overwrite each other without conflict detection. Update operations should require an `expected_version` parameter.

**Example Scenario**:
```
Initial: actor A = {value: 100, version: 5}

Concurrent updates:
t0: Client C1 reads: {value: 100, version: 5}
t1: Client C2 reads: {value: 100, version: 5}
t2: C1 sends: Update(A, value=150)  // no version check!
t3: C2 sends: Update(A, value=200)  // no version check!
t4: Both succeed, but C1's update lost
t5: Final state: {value: 200, version: 7}

C1's update (100→150) was silently overwritten.

With CAS:
t2: C1 sends: Update(A, value=150, expected_version=5)
t3: C1 succeeds → version=6
t4: C2 sends: Update(A, value=200, expected_version=5)
t5: C2 fails → "Version mismatch: expected 5, got 6"
t6: C2 retries with latest version
```

**Recommendation**:
- Enforce `expected_version` in Update operations
- Return version conflict errors
- Support conditional updates

---

### 11. Safe Membership Changes ⚠️

**Property**: Adding or removing nodes from the cluster should not compromise safety.

**Definition**:
Currently, `cluster_size` is updated atomically, but there's no joint consensus phase. This can violate safety during transitions. Example: Going from 3 nodes to 5 nodes changes quorum from 2 to 3. If the change is not atomic across all nodes, two different quorums can form.

**Example Scenario**:
```
Initial cluster: {N1, N2, N3}, quorum=2
Target cluster: {N1, N2, N3, N4, N5}, quorum=3

Transition:
t0: N1, N2 see cluster_size=3, quorum=2
t1: N3, N4, N5 see cluster_size=5, quorum=3
t2: N1, N2 elect N1 as leader (2 votes, old quorum)
t3: N3, N4, N5 elect N3 as leader (3 votes, new quorum)
→ Two leaders in same term! Safety violated.

Joint consensus fix:
t0: Transition to joint mode: C_old ∪ C_new
    Require quorum in BOTH old (2 of {N1,N2,N3})
                AND new (3 of {N1,N2,N3,N4,N5})
t1: Once joint committed, transition to C_new only
    Now everyone uses quorum=3
```

**Recommendation**:
- Implement Raft-style joint consensus for membership changes
- Or: Use a fixed cluster size with manual reconfiguration and downtime

---

### 12. Monotonic Reads ⚠️

**Property**: A client's reads should never go backward in time (session consistency).

**Definition**:
If a client reads value V1 at time t1, all subsequent reads by that client should return V1 or a later value, never an earlier value. This requires:
- Tracking per-client "last read timestamp/sequence"
- Ensuring subsequent reads are from replicas at least as up-to-date

**Example Scenario**:
```
Client C1:
t0: Read from Leader → actor A = {version: 10, seq: 100}
t1: Read from Follower F1 → actor A = {version: 8, seq: 95}
    Violates monotonic reads! Version went backward: 10 → 8

With monotonic reads:
t0: Read from Leader → seq=100, cache in session
t1: Read from F1 → F1 at seq=95, too stale
    Wait or redirect to replica at seq ≥ 100
```

**Recommendation**:
- Implement session tokens tracking last-read sequence
- Include `min_sequence` parameter in read requests
- Followers reject reads if below client's last-seen sequence

---

### 13. Causal Consistency ⚠️

**Property**: If operation A causally precedes operation B, all nodes observe A before B.

**Definition**:
Currently using wall-clock timestamps (`chrono::Utc::now()`), which don't capture causality. Concurrent operations might be ordered incorrectly across nodes due to clock skew. Vector clocks or Lamport timestamps provide happens-before relationships.

**Example Scenario**:
```
t0: Node1 (clock=10:00:00.000) writes: Create(A)
t1: Node2 (clock=09:59:59.900) writes: Create(B)
    Node2's clock is 100ms behind

With wall-clock ordering:
Sorted events: [Create(B, ts=09:59:59.900), Create(A, ts=10:00:00.000)]

But actual causality might be opposite!
If Create(A) happened-before Create(B), we've violated causality.

With vector clocks:
Node1: Create(A) → VC={N1:1, N2:0}
Node2: Create(B) → VC={N1:0, N2:1}
No causal relationship, can apply in any order ✓

Or if B reads A first:
Node1: Create(A) → VC={N1:1}
Node2: Read(A) → receives VC={N1:1}, merges → VC={N1:1, N2:0}
Node2: Create(B) → VC={N1:1, N2:1}
Now Create(A) causally precedes Create(B), enforced everywhere
```

**Recommendation**:
- Replace wall-clock timestamps with Lamport timestamps or vector clocks
- Track causal dependencies between operations
- Ensure replicas apply operations respecting causality

---

## Summary

### Implemented Properties: 6
1. ✅ Split-Brain Write Protection
2. ✅ CDC Read Consistency (Majority/All)
3. ✅ Sequence Monotonicity
4. ✅ Circular Buffer Bounds
5. ✅ Single Leader per Term
6. ✅ Quorum-based Election

### Recommended Properties: 7
7. ⚠️ Zombie Leader Protection
8. ⚠️ Idempotency
9. ⚠️ Linearizable Reads
10. ⚠️ Lost Update Prevention
11. ⚠️ Safe Membership Changes
12. ⚠️ Monotonic Reads
13. ⚠️ Causal Consistency

---

## Testing Checklist

For each implemented property, maintain tests that demonstrate:
- ✅ Normal operation
- ✅ Failure scenario that would violate the property
- ✅ Recovery mechanism that preserves the property

For each recommended property, plan tests that would verify:
- ⚠️ Fault injection scenario
- ⚠️ Expected behavior
- ⚠️ Invariant validation

---

## Formal Verification and Verifiability

### Can These Properties Be Verified in Coq?

Yes, most of these properties are formally verifiable in Coq (or similar proof assistants), though they vary in difficulty. This section outlines which properties are verifiable and provides guidance for future formal verification work.

### Highly Verifiable Properties ✓

**Already Implemented:**

1. **Split-Brain Write Protection** - Excellent candidate for formal verification. Can be modeled as: `∀ events, term(e) < current_term → e ∉ committed_log`. This is similar to Raft's log matching property, which has been formally verified in systems like [Verdi](https://github.com/uwplse/verdi).

2. **Sequence Monotonicity** - Trivial to verify: `∀ e1 e2, append_order(e1, e2) → seq(e1) < seq(e2)`. This is a simple state machine invariant.

3. **Single Leader per Term** - Classic Raft property, already formalized in multiple proof systems: `∀ t n1 n2, leader(n1,t) ∧ leader(n2,t) → n1 = n2`.

4. **Quorum-based Election** - Pure mathematical property (quorum intersection): `∀ Q1 Q2, is_quorum(Q1) ∧ is_quorum(Q2) → Q1 ∩ Q2 ≠ ∅`. Easily provable in Coq.

5. **Circular Buffer Bounds** - Simple data structure invariant: `|buffer| ≤ buffer_size`. Straightforward to verify.

6. **CDC Read Consistency** - Verifiable as quorum property: `∀ e ∈ visible_events, |{n : acked(n,e)}| ≥ quorum_size`. Can be proven using quorum intersection.

**Recommended:**

7. **Lost Update Prevention** - State machine property: `update(k,v,exp_ver) succeeds → current_version(k) = exp_ver`. Clean specification makes verification straightforward.

8. **Idempotency** - Verifiable as: `∀ req_id, execute(req_id) = execute(req_id) ∘ execute(req_id)`. Note: bounded cache requires additional liveness assumptions.

11. **Safe Membership Changes** - Joint consensus is formalized in Raft proofs. Requires modeling configuration transitions.

13. **Causal Consistency** - Verifiable with happens-before relation: `a →ₕb b → ∀ nodes, observe(a) < observe(b)`. Requires vector clock or Lamport timestamp model.

### Moderately Challenging Properties

7. **Zombie Leader Protection** - Verifiable, but requires modeling fencing tokens and storage layer interactions. Need to prove: `∀ writes, token(write) < current_token → rejected`. Requires refinement across multiple abstraction layers.

8. **Idempotency** - Need to model cache eviction policy (bounded cache). Unbounded cache is trivial, but bounded requires careful liveness assumptions about cache size vs. request rate.

9. **Linearizable Reads** - **Challenging**. Requires modeling real-time ordering and identifying the linearization point. Possible but complex. See [Iris](https://iris-project.org/) for examples of linearizability proofs in separation logic.

12. **Monotonic Reads** - Need session model with client state: `∀ client c, read_seq(c,t1) ≤ read_seq(c,t2) when t1 < t2`. Requires modeling client sessions and replica state visibility.

### The Reality Gap: What Coq Proves vs. What It Doesn't

**What Coq Proves**:
- Properties of your **formal model** (state machine, transitions, message passing)
- Safety invariants across all reachable states
- Correctness of protocol logic

**What Coq Doesn't Prove**:
- Your Rust implementation matches the formal model (need refinement proofs or code extraction)
- Network actually delivers messages (requires partial synchrony assumptions as axioms)
- Clocks behave reasonably (need to axiomatize timing properties)
- Absence of Byzantine failures (need different model for BFT)
- No bugs in: tokio runtime, OS kernel, hardware

### Existing Verified Distributed Systems

Several projects bridge the verification gap:

- **[Verdi](https://github.com/uwplse/verdi)** - Coq framework for verifying distributed systems. Raft is fully formalized here.
- **[IronFleet](https://github.com/Microsoft/Ironcleet)** - Verified distributed systems (Paxos, chain replication) with refinement to executable code.
- **[Ivy](https://github.com/kenmcmil/ivy)** - Decidable verification for distributed protocols using first-order logic.
- **[TLA+](https://lamport.azurewebsites.net/tla/tla.html)** - Model checking (not proof, but excellent for finding bugs). Used by AWS, Microsoft, etc.

### Practical Recommendation: Hybrid Approach

For the actor store, a **hybrid verification approach** works best:

#### 1. Formal Model in Coq

```coq
(* High-level protocol properties *)
Theorem split_brain_safety :
  ∀ (st : system_state) (e : event),
    term(e) < current_term(st) →
    e ∉ committed_events(st).

(* Prove key invariants hold across all reachable states *)
Theorem state_machine_safety :
  ∀ st, reachable(st) →
    single_leader_per_term(st) ∧
    quorum_committed(st) ∧
    no_split_brain(st).
```

#### 2. Implementation Strategy

1. **Model** core protocol in Coq (consensus, replication, CDC)
2. **Prove** safety properties (no divergence, no data loss)
3. **Extract** to OCaml (or use as reference implementation)
4. **Manual refinement** to Rust for performance
5. **Property-based testing** (QuickCheck-style) to validate Rust matches model

#### 3. Verification Workflow

```
┌─────────────┐
│ Coq Model   │ ← Prove safety properties
└──────┬──────┘
       │
       ├─→ Extract to OCaml (verified)
       │
       └─→ Reference for Rust implementation
            │
            ├─→ Property-based tests (proptest)
            │   Validate Rust behavior matches model
            │
            └─→ Integration tests (Jepsen-style)
                Chaos engineering, fault injection
```

### Next Steps for Verification

Choose based on your goals:

1. **Proving Correctness**:
   - Create a Coq formalization of the actor store protocol
   - Prove safety properties (already-implemented invariants 1-6)
   - Consider extraction or refinement to implementation

2. **Finding Bugs**:
   - Write property-based tests in Rust using `proptest`
   - Mirror the formal properties in test assertions
   - Use fault injection and chaos engineering

3. **Documentation**:
   - Write TLA+ specification as executable documentation
   - Model check specific scenarios
   - Generate state space diagrams

### Example: Property-Based Test for Split-Brain

```rust
// Using proptest to validate split-brain protection
proptest! {
    #[test]
    fn split_brain_writes_rolled_back(
        old_term in 1u64..100,
        new_term in 101u64..200,
        num_writes in 1usize..10
    ) {
        // Setup: Node thinks it's leader in old_term
        let node = setup_node(old_term);

        // Execute: Accept writes in stale term
        let writes = write_batch(&node, num_writes);

        // Rejoin: Discover new term
        node.rejoin_cluster(new_term);

        // Verify: All stale writes rolled back
        for write in writes {
            prop_assert!(!node.is_committed(&write));
        }
    }
}
```

### Resources for Getting Started

- **Coq**: [Software Foundations](https://softwarefoundations.cis.upenn.edu/) - Learn Coq and verified programming
- **Verdi**: [Tutorial](https://github.com/uwplse/verdi/wiki) - Framework for distributed systems verification
- **TLA+**: [Learn TLA+](https://learntla.com/) - Model checking for distributed systems
- **Jepsen**: [Consistency Models](https://jepsen.io/consistency) - Testing distributed systems
- **Proptest**: [Rust Property Testing](https://proptest-rs.github.io/proptest/) - QuickCheck for Rust

---

## References

- [Jepsen: Consistency Models](https://jepsen.io/consistency)
- [Raft Consensus Algorithm](https://raft.github.io/)
- [Google Spanner: TrueTime](https://cloud.google.com/spanner/docs/true-time-external-consistency)
- [Designing Data-Intensive Applications, Martin Kleppmann](https://dataintensive.net/)
- [Verdi: Formally Verifying Distributed Systems](https://github.com/uwplse/verdi)
- [IronFleet: Proving Practical Distributed Systems Correct](https://github.com/Microsoft/Ironcleet)
- [TLA+: Specification and Verification](https://lamport.azurewebsites.net/tla/tla.html)
- [Software Foundations (Coq)](https://softwarefoundations.cis.upenn.edu/)