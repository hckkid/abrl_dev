# Distributed Systems Reading List

A curated collection of books, papers, and resources covering the distributed systems properties and invariants documented in [invariants.md](./invariants.md).

---

## Essential Reading

### 1. Designing Data-Intensive Applications
**by Martin Kleppmann (2017)**

**The definitive modern book** - covers almost all our properties:

- **Chapter 5 (Replication)**: Leader election, split-brain, failover, quorum replication, read consistency levels
- **Chapter 6 (Partitioning)**: Distributed transactions, linearizability
- **Chapter 7 (Transactions)**: Lost updates, CAS, isolation levels, idempotency
- **Chapter 8 (Distributed Systems)**: Fencing tokens, monotonic reads, causal consistency, happens-before
- **Chapter 9 (Consistency)**: Linearizability vs eventual consistency, session guarantees
- **Chapter 11 (Stream Processing)**: CDC patterns, event ordering, exactly-once semantics

🎯 **Best for**: Practical understanding with real-world examples. Essential reading.

📚 **Link**: https://dataintensive.net/

---

### 2. Database Internals
**by Alex Petrov (2019)**

**Implementation-focused**:

- **Part II (Distributed Systems)**: Consensus (Raft/Paxos), leader election, log replication
- **Chapter 12 (Leader Election)**: Terms, quorum-based election, split-brain prevention
- **Chapter 13 (Replication)**: State machine replication, CDC implementation
- **Chapter 14 (Anti-Entropy)**: Failure detection, membership changes

🎯 **Best for**: How systems like Cassandra, etcd, Kafka actually implement these properties.

📚 **Link**: https://www.databass.dev/

---

## Consensus & Fault Tolerance

### 3. Understanding Distributed Systems
**by Roberto Vitillo (2022)**

**Modern and accessible**:

- Consistency models (linearizable, sequential, causal, eventual)
- Replication strategies and trade-offs
- Consensus protocols (Raft explained well)
- Failure detection and recovery
- Membership and coordination

🎯 **Best for**: Quick, practical overview. Great first book before DDIA.

📚 **Link**: https://understandingdistributed.systems/

---

### 4. Introduction to Reliable and Secure Distributed Programming
**by Christian Cachin, Rachid Guerraoui, Luís Rodrigues (2011)**

**Theoretical but rigorous**:

- Formal models for distributed systems
- Broadcast protocols and ordering guarantees
- Consensus algorithms with proofs
- Fault models (crash, Byzantine)
- Quorum systems and intersection properties

🎯 **Best for**: Understanding the formal foundations and proofs.

📚 **Link**: https://www.distributedprogramming.net/

---

## Formal Methods & Verification

### 5. Specifying Systems
**by Leslie Lamport (2002)**

**TLA+ and formal specification**:

- Temporal logic for system properties
- Model checking distributed protocols
- Specifying safety and liveness
- Verifying consensus algorithms

🎯 **Best for**: Learning to formally specify and verify properties in TLA+.

📚 **Link**: https://lamport.azurewebsites.net/tla/book.html

---

### 6. Verified Software: Theories, Tools, Experiments
**Various authors (Springer LNCS series)**

**Formal verification in practice**:

- IronFleet verification case studies
- Verdi framework examples
- Refinement proofs for distributed systems

🎯 **Best for**: Research-level verification techniques.

---

## Classic Textbooks

### 7. Distributed Systems (3rd Edition)
**by Maarten van Steen and Andrew S. Tanenbaum (2017)**

**Comprehensive textbook**:

- **Chapters 6-8**: Synchronization, consistency, replication
- Logical clocks, vector clocks, causal ordering
- Consensus and mutual exclusion
- Fault tolerance and recovery

🎯 **Best for**: Academic course material, comprehensive coverage.

📚 **Link**: https://www.distributed-systems.net/

---

### 8. Replication: Theory and Practice
**Edited by Bernadette Charron-Bost, Fernando Pedone, André Schiper (2010)**

**Academic anthology**:

- State machine replication
- Atomic broadcast protocols
- Byzantine fault tolerance
- Consistency models and theory

🎯 **Best for**: Deep dive into replication research.

📚 **Link**: Springer LNCS Vol 5959

---

## Practical Systems

### 9. Streaming Systems
**by Tyler Akidau, Slava Chernyak, Reuven Lax (2018)**

**Stream processing and CDC**:

- Event time vs processing time
- Exactly-once semantics and idempotency
- Watermarks and completeness
- Windowing and ordering

🎯 **Best for**: CDC, event streaming, and time-based properties.

📚 **Link**: https://www.oreilly.com/library/view/streaming-systems/9781491983867/

---

## Property-Specific Recommendations

### Split-Brain & Failover
- **Designing Data-Intensive Applications** - Chapter 5
- **Database Internals** - Chapter 12
- **Understanding Distributed Systems** - Chapters on Replication

### CDC & Consistency
- **Designing Data-Intensive Applications** - Chapters 5, 9, 11
- **Streaming Systems** - Chapters 1-3
- **Database Internals** - Chapter 13

### Linearizability & Read Consistency
- **Designing Data-Intensive Applications** - Chapter 9
- **Introduction to Reliable Distributed Programming** - Chapter 4
- **Distributed Systems (Tanenbaum)** - Chapter 7

### Causal Consistency & Vector Clocks
- **Designing Data-Intensive Applications** - Chapter 9
- **Distributed Systems (Tanenbaum)** - Chapter 6
- Lamport's "Time, Clocks" paper (see below)

### Membership Changes & Reconfiguration
- **Database Internals** - Chapter 12
- Raft paper (see below)
- **Introduction to Reliable Distributed Programming** - Chapter 5

### Formal Verification
- **Specifying Systems** - TLA+ approach
- **Introduction to Reliable Distributed Programming** - Formal proofs
- Verdi/IronFleet papers (see below)

---

## Essential Papers

### Consensus Algorithms

**Raft**
- "In Search of an Understandable Consensus Algorithm"
- Diego Ongaro, John Ousterhout (2014)
- 📄 https://raft.github.io/raft.pdf

**Paxos**
- "Paxos Made Simple"
- Leslie Lamport (2001)
- 📄 https://lamport.azurewebsites.net/pubs/paxos-simple.pdf

### Consistency Models

**Linearizability**
- "Linearizability: A Correctness Condition for Concurrent Objects"
- Maurice P. Herlihy, Jeannette M. Wing (1990)
- 📄 https://cs.brown.edu/~mph/HerlihyW90/p463-herlihy.pdf

**Causal Consistency**
- "Time, Clocks, and the Ordering of Events in a Distributed System"
- Leslie Lamport (1978)
- 📄 https://lamport.azurewebsites.net/pubs/time-clocks.pdf

### Replication & Fault Tolerance

**Fencing Tokens**
- "How to Build a Highly Available System Using Consensus"
- Butler W. Lampson (1996)
- 📄 https://www.microsoft.com/en-us/research/publication/how-to-build-a-highly-available-system-using-consensus/

**State Machine Replication**
- "Implementing Fault-Tolerant Services Using the State Machine Approach"
- Fred B. Schneider (1990)
- 📄 https://www.cs.cornell.edu/fbs/publications/SMSurvey.pdf

### Formal Verification

**Verdi**
- "Verdi: A Framework for Implementing and Formally Verifying Distributed Systems"
- James R. Wilcox et al. (2015)
- 📄 https://homes.cs.washington.edu/~jrw12/verdi.pdf

**IronFleet**
- "IronFleet: Proving Practical Distributed Systems Correct"
- Chris Hawblitzel et al. (2015)
- 📄 https://www.microsoft.com/en-us/research/publication/ironfleet-proving-practical-distributed-systems-correct/

### Modern Systems

**Google Spanner**
- "Spanner: Google's Globally-Distributed Database"
- James C. Corbett et al. (2012)
- 📄 https://research.google/pubs/pub39966/

**Apache Kafka**
- "Kafka: a Distributed Messaging System for Log Processing"
- Jay Kreps et al. (2011)
- 📄 https://research.cs.wisc.edu/wind/Publications/kafka.pdf

---

## Recommended Reading Order

### For the Actor Store Project

#### Phase 1: Foundation (2-3 weeks)
1. **Understanding Distributed Systems** by Roberto Vitillo
   - Quick overview of all concepts
   - Understand the landscape

2. **Designing Data-Intensive Applications** by Martin Kleppmann
   - Deep dive into all properties
   - Real-world trade-offs

#### Phase 2: Implementation (2-3 weeks)
3. **Database Internals** by Alex Petrov
   - How real systems implement these patterns
   - CDC and replication internals

4. **Raft Paper** - "In Search of an Understandable Consensus Algorithm"
   - Understand consensus in detail
   - Reference implementation patterns

#### Phase 3: Verification (if pursuing formal methods)
5. **Specifying Systems** by Leslie Lamport
   - Learn TLA+ for model checking
   - Specify actor store properties

6. **Introduction to Reliable Distributed Programming** by Cachin et al.
   - Formal proofs and theory
   - Rigorous foundations

#### Phase 4: Deep Dive (ongoing)
7. **Streaming Systems** (for CDC focus)
8. **Distributed Systems** by Tanenbaum (comprehensive reference)
9. **Selected papers** based on specific properties you're implementing

---

## Online Courses & Resources

### Interactive Learning

**MIT 6.824: Distributed Systems**
- Video lectures by Robert Morris
- Lab assignments implement Raft, KV store, sharded DB
- 📺 https://pdos.csail.mit.edu/6.824/

**Jepsen Analyses**
- Real-world testing of distributed databases
- Property violations in production systems
- 🔬 https://jepsen.io/analyses

**Aphyr's Blog (Kyle Kingsbury)**
- Distributed systems consistency analysis
- Clear explanations of subtle bugs
- 📝 https://aphyr.com/

### Formal Methods

**Learn TLA+**
- Interactive TLA+ tutorial
- Model checking exercises
- 🎓 https://learntla.com/

**Software Foundations**
- Coq-based verification course
- Learn proof assistants
- 🎓 https://softwarefoundations.cis.upenn.edu/

### Testing & Verification

**Jepsen Testing Framework**
- Chaos engineering for distributed systems
- Property-based testing
- 🛠️ https://github.com/jepsen-io/jepsen

**FoundationDB Testing**
- Deterministic simulation testing
- 📝 https://apple.github.io/foundationdb/testing.html

---

## Blogs & Active Researchers

### Essential Blogs

- **Martin Kleppmann** - https://martin.kleppmann.com/
- **Kyle Kingsbury (Aphyr)** - https://aphyr.com/
- **Dan Luu** - https://danluu.com/
- **Marc Brooker (AWS)** - https://brooker.co.za/blog/
- **Peter Bailis** - http://www.bailis.org/blog/

### Research Groups

- **MIT PDOS** - https://pdos.csail.mit.edu/
- **UW PLSE** - https://uwplse.org/ (Verdi, IronFleet)
- **CMU PDL** - https://www.pdl.cmu.edu/
- **Berkeley RISELab** - https://rise.cs.berkeley.edu/

---

## Property Coverage Matrix

| Property | Primary Book | Chapter | Papers |
|----------|--------------|---------|--------|
| Split-Brain Write Protection | DDIA | Ch 5 | Raft, Paxos |
| CDC Read Consistency | DDIA, Streaming Systems | Ch 11, Ch 1-3 | Kafka |
| Sequence Monotonicity | Database Internals | Ch 13 | State Machine Replication |
| Circular Buffer Bounds | Streaming Systems | Ch 5 | - |
| Single Leader per Term | DDIA, Database Internals | Ch 5, Ch 12 | Raft |
| Quorum-based Election | DDIA | Ch 5 | Raft, Paxos |
| Zombie Leader Protection | DDIA | Ch 8 | Lampson consensus |
| Idempotency | DDIA, Streaming Systems | Ch 11 | Exactly-once semantics |
| Linearizable Reads | DDIA | Ch 9 | Herlihy & Wing |
| Lost Update Prevention | DDIA | Ch 7 | Optimistic Concurrency |
| Safe Membership Changes | Database Internals | Ch 12 | Raft (§6) |
| Monotonic Reads | DDIA | Ch 9 | Session guarantees |
| Causal Consistency | DDIA, Cachin | Ch 9, Ch 4 | Lamport clocks |

---

## Quick Reference Guide

### For Bug Fixing
1. Search [Jepsen Analyses](https://jepsen.io/analyses) for similar issues
2. Check relevant DDIA chapter
3. Read original paper for formal specification

### For Implementation
1. DDIA for conceptual understanding
2. Database Internals for implementation patterns
3. Reference implementation (Raft, etcd, Kafka source)

### For Verification
1. Specify property in TLA+ (Specifying Systems)
2. Model check with TLC
3. Consider Coq formalization (Software Foundations)

### For Research
1. Original papers (listed above)
2. Recent conference proceedings (OSDI, SOSP, NSDI)
3. Research group publications

---

## Updates & Maintenance

This reading list focuses on timeless principles and foundational work. For cutting-edge developments:

- **Conferences**: OSDI, SOSP, NSDI, VLDB, SIGMOD
- **Workshops**: PaPoC, LADIS, HotOS
- **ArXiv**: https://arxiv.org/list/cs.DC/recent (Distributed Computing)

**Last Updated**: 2025-01-04

---

## Contributing

Found a great resource? This reading list lives alongside the [invariants.md](./invariants.md) documentation. Additions welcome for:

- New books covering distributed systems properties
- Important papers on consensus, consistency, or replication
- Practical implementation guides
- Formal verification resources

------

Essential Reading

1. Designing Data-Intensive Applications by Martin Kleppmann (2017)

The definitive modern book - covers almost all our properties:

- Ch 5 (Replication): Leader election, split-brain, failover, quorum replication, read consistency levels
- Ch 6 (Partitioning): Distributed transactions, linearizability
- Ch 7 (Transactions): Lost updates, CAS, isolation levels, idempotency
- Ch 8 (Distributed Systems): Fencing tokens, monotonic reads, causal consistency, happens-before
- Ch 9 (Consistency): Linearizability vs eventual consistency, session guarantees
- Ch 11 (Stream Processing): CDC patterns, event ordering, exactly-once semantics

🎯 Best for: Practical understanding with real-world examples. Essential reading.

2. Database Internals by Alex Petrov (2019)

Implementation-focused:

- Part II (Distributed Systems): Consensus (Raft/Paxos), leader election, log replication
- Ch 12 (Leader Election): Terms, quorum-based election, split-brain prevention
- Ch 13 (Replication): State machine replication, CDC implementation
- Ch 14 (Anti-Entropy): Failure detection, membership changes

🎯 Best for: How systems like Cassandra, etcd, Kafka actually implement these properties.

Consensus & Fault Tolerance

3. Understanding Distributed Systems by Roberto Vitillo (2022)

Modern and accessible:

- Consistency models (linearizable, sequential, causal, eventual)
- Replication strategies and trade-offs
- Consensus protocols (Raft explained well)
- Failure detection and recovery
- Membership and coordination

🎯 Best for: Quick, practical overview. Great first book before DDIA.

4. Introduction to Reliable and Secure Distributed Programming by Cachin, Guerraoui, Rodrigues (2011)

Theoretical but rigorous:

- Formal models for distributed systems
- Broadcast protocols and ordering guarantees
- Consensus algorithms with proofs
- Fault models (crash, Byzantine)
- Quorum systems and intersection properties

🎯 Best for: Understanding the formal foundations and proofs.

Formal Methods & Verification

5. Specifying Systems by Leslie Lamport (2002)

TLA+ and formal specification:

- Temporal logic for system properties
- Model checking distributed protocols
- Specifying safety and liveness
- Verifying consensus algorithms

🎯 Best for: Learning to formally specify and verify properties in TLA+.

6. Verified Software: Theories, Tools, Experiments (Various authors)

Formal verification in practice:

- IronFleet verification case studies
- Verdi framework examples
- Refinement proofs for distributed systems

🎯 Best for: Research-level verification techniques.

Classic Textbooks

7. Distributed Systems by Maarten van Steen and Andrew S. Tanenbaum (3rd ed, 2017)

Comprehensive textbook:

- Ch 6-8: Synchronization, consistency, replication
- Logical clocks, vector clocks, causal ordering
- Consensus and mutual exclusion
- Fault tolerance and recovery

🎯 Best for: Academic course material, comprehensive coverage.

8. Replication: Theory and Practice edited by Charron-Bost, Pedone, Schiper (2010)

Academic anthology:

- State machine replication
- Atomic broadcast protocols
- Byzantine fault tolerance
- Consistency models and theory

🎯 Best for: Deep dive into replication research.

Practical Systems

9. Streaming Systems by Tyler Akidau, Slava Chernyak, Reuven Lax (2018)

Stream processing and CDC:

- Event time vs processing time
- Exactly-once semantics and idempotency
- Watermarks and completeness
- Windowing and ordering

🎯 Best for: CDC, event streaming, and time-based properties.

Property-Specific Recommendations

For Split-Brain & Failover:
- DDIA Ch 5, Database Internals Ch 12

For CDC & Consistency:
- DDIA Ch 5, 9, 11; Streaming Systems Ch 1-3

For Linearizability & Read Consistency:
- DDIA Ch 9; Cachin et al. Ch 4

For Causal Consistency & Vector Clocks:
- DDIA Ch 9; Distributed Systems (Tanenbaum) Ch 6

For Membership Changes & Reconfiguration:
- Database Internals Ch 12; Raft paper directly

For Formal Verification:
- Specifying Systems (TLA+); Cachin et al. (proofs)

Papers (Essential)

If you want to go deeper, read the original papers:

- Raft: "In Search of an Understandable Consensus Algorithm" (Ongaro & Ousterhout, 2014)
- Paxos: "Paxos Made Simple" (Lamport, 2001)
- Consistency Models: "Linearizability: A Correctness Condition" (Herlihy & Wing, 1990)
- Causal Consistency: "Time, Clocks and Ordering of Events" (Lamport, 1978)
- Fencing Tokens: "How to Build a Highly Available System" (Gray & Lamport, 2006)

Recommended Reading Order

For your actor store project:

1. Start: "Understanding Distributed Systems" (quick overview)
2. Core: "Designing Data-Intensive Applications" (comprehensive)
3. Implementation: "Database Internals" (how real systems do it)
4. Verification: "Specifying Systems" (if pursuing TLA+/Coq)
5. Deep Dive: Raft paper + relevant chapters from academic books
