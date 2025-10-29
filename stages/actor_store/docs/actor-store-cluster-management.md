# Actor Store - Cluster Management and Node Discovery

## Overview

This document provides detailed specifications for cluster formation, node discovery, node lifecycle management, and the role-based architecture for Actor Store nodes. It covers how to bootstrap a single-node cluster, add follower nodes dynamically, manage node identities independently of network addresses, and implement the three-tier node role system (Primary, Secondary, Read-Only).

---

## Part 1: Node Identity and Addressing

### 1.1 Node Identity Model

Each node in the cluster has a unique, immutable identity separate from its network addresses. This separation allows nodes to change their network location without affecting cluster membership and enables nodes to expose multiple network interfaces simultaneously.

**Node Identifier Structure:**

```rust
NodeId = UUID (128-bit)
// Generated once at node initialization
// Persists across restarts in node metadata file
// Example: "550e8400-e29b-41d4-a716-446655440000"

NodeIdentity = {
    id: NodeId,
    name: String,              // Human-readable name (e.g., "node-us-east-1a")
    role: NodeRole,            // PRIMARY | SECONDARY | READ_ONLY
    version: String,           // Software version
    metadata: Map<String, String>  // Custom key-value metadata
}

NodeRole = PRIMARY | SECONDARY | READ_ONLY

PRIMARY:
  - Can accept write operations
  - Participates in leader election
  - Serves read operations
  - Replicates to secondary nodes
  - Can be demoted to SECONDARY

SECONDARY:
  - Cannot accept write operations (returns redirect or error)
  - Participates in leader election
  - Can be elected as leader (becomes PRIMARY)
  - Serves read operations (eventual consistency)
  - Replicates data from PRIMARY
  - Can be promoted to PRIMARY manually or during failover

READ_ONLY:
  - Cannot accept write operations
  - NEVER participates in leader election
  - Cannot become PRIMARY under any circumstance
  - Serves read operations (eventual consistency)
  - Replicates data from PRIMARY
  - Useful for read-heavy workloads and geographic distribution
```

**Network Addresses:**

Each node exposes multiple network endpoints for different protocols:

```rust
NodeAddresses = {
    node_id: NodeId,
    
    // RPC endpoint (gRPC for internal cluster communication)
    rpc_address: SocketAddr,        // e.g., "10.0.1.5:7001"
    
    // HTTP API endpoint (REST API for clients)
    http_address: SocketAddr,       // e.g., "10.0.1.5:8080"
    
    // Admin endpoint (cluster management operations)
    admin_address: SocketAddr,      // e.g., "10.0.1.5:9090"
    
    // Optional: multiple addresses for multi-homed hosts
    additional_rpc: Vec<SocketAddr>,
    additional_http: Vec<SocketAddr>,
}
```

**Why Separate Identity from Address:**

1. **Mobility**: Nodes can change IP addresses (cloud autoscaling, network reconfiguration) without losing cluster membership
2. **Multi-Protocol**: A single node serves multiple protocols on different ports
3. **Multi-Interface**: Nodes with multiple network interfaces can advertise all addresses
4. **DNS Independence**: Cluster works without DNS, but can also use DNS names when available
5. **Testing**: Easier to run multiple nodes on localhost with different ports

### 1.2 Node Metadata Persistence

Each node maintains a local metadata file that persists across restarts:

```toml
# /var/lib/actor-store/node-metadata.toml

[identity]
id = "550e8400-e29b-41d4-a716-446655440000"
name = "node-us-east-1a"
role = "SECONDARY"
version = "0.1.0"
initialized_at = "2025-01-15T10:30:00Z"

[cluster]
cluster_id = "production-cluster-alpha"
cluster_secret_hash = "sha256:abcd1234..."  # For authentication

[addresses]
rpc_address = "10.0.1.5:7001"
http_address = "10.0.1.5:8080"
admin_address = "10.0.1.5:9090"

[metadata]
datacenter = "us-east-1"
availability_zone = "us-east-1a"
instance_type = "m5.2xlarge"
```

**Node Initialization:**

When a node starts for the first time:

```rust
fn initialize_node(config: NodeConfig) -> Result<NodeIdentity> {
    let metadata_path = config.data_dir.join("node-metadata.toml");
    
    if metadata_path.exists() {
        // Load existing identity
        load_node_metadata(&metadata_path)
    } else {
        // Generate new identity
        let node_id = NodeId::new_v4();  // UUID v4
        let identity = NodeIdentity {
            id: node_id,
            name: config.node_name.unwrap_or_else(|| 
                format!("node-{}", &node_id.to_string()[..8])
            ),
            role: config.initial_role,
            version: env!("CARGO_PKG_VERSION").to_string(),
            metadata: config.metadata,
        };
        
        // Persist to disk
        save_node_metadata(&metadata_path, &identity)?;
        
        Ok(identity)
    }
}
```

---

## Part 2: Cluster Discovery

### 2.1 Bootstrap Methods

The Actor Store supports multiple bootstrap methods for initial cluster formation and node discovery:

**Static Configuration (seed nodes):**

```toml
# /etc/actor-store/cluster.toml

[cluster]
name = "production-cluster-alpha"
replication_factor = 3

# Seed nodes for initial discovery
[[seeds]]
node_id = "550e8400-e29b-41d4-a716-446655440000"
rpc_address = "10.0.1.5:7001"

[[seeds]]
node_id = "7c9e6679-7425-40de-944b-e07fc1f90ae7"
rpc_address = "10.0.1.6:7001"

[[seeds]]
node_id = "9f8e3e95-7ae7-4f8e-9e3e-7f8e9e3e7f8e"
rpc_address = "10.0.1.7:7001"
```

**DNS-Based Discovery:**

```toml
[discovery]
method = "dns"
dns_name = "actor-store.internal.example.com"
# Performs SRV lookup: _actor-store._tcp.internal.example.com
# Or A/AAAA lookup with standard port
```

**Kubernetes Service Discovery:**

```toml
[discovery]
method = "kubernetes"
namespace = "production"
service_name = "actor-store"
label_selector = "app=actor-store,tier=storage"
```

**Consul/etcd Service Registry:**

```toml
[discovery]
method = "consul"
consul_address = "consul.service.consul:8500"
service_name = "actor-store"
```

**File-Based Discovery (for development/testing):**

```toml
[discovery]
method = "file"
registry_file = "/tmp/actor-store-cluster.json"
```

### 2.2 Single-Node Bootstrap

Starting the very first node creates a single-node cluster:

**Command Line:**

```bash
# Initialize and start first node
actor-store start \
  --data-dir /var/lib/actor-store \
  --node-name node-primary-1 \
  --role PRIMARY \
  --rpc-address 0.0.0.0:7001 \
  --http-address 0.0.0.0:8080 \
  --admin-address 0.0.0.0:9090 \
  --bootstrap-cluster

# The --bootstrap-cluster flag indicates this is the first node
# Without it, the node expects to join an existing cluster
```

**What Happens During Bootstrap:**

```rust
fn bootstrap_single_node_cluster(identity: NodeIdentity, addresses: NodeAddresses) -> Result<Cluster> {
    // 1. Generate cluster ID
    let cluster_id = ClusterId::new_v4();
    
    // 2. Initialize Raft with single member
    let raft_config = RaftConfig {
        cluster_id: cluster_id.clone(),
        node_id: identity.id,
        initial_members: vec![Member {
            node_id: identity.id,
            addresses: addresses.clone(),
            role: NodeRole::PRIMARY,
        }],
        election_timeout: Duration::from_millis(300),
        heartbeat_interval: Duration::from_millis(100),
    };
    
    let raft = RaftNode::new(raft_config)?;
    
    // 3. Elect self as leader (trivial election with single member)
    raft.bootstrap_as_leader()?;
    
    // 4. Initialize empty actor store
    let store = ActorStore::new(identity.id)?;
    
    // 5. Start services
    let cluster = Cluster {
        id: cluster_id,
        identity,
        addresses,
        raft,
        store,
        members: HashMap::from([(identity.id, MemberInfo {
            identity: identity.clone(),
            addresses: addresses.clone(),
            state: MemberState::ACTIVE,
            last_heartbeat: Instant::now(),
        })]),
    };
    
    // 6. Persist cluster membership
    cluster.save_membership_to_disk()?;
    
    log::info!("Bootstrapped single-node cluster: {}", cluster_id);
    
    Ok(cluster)
}
```

### 2.3 Adding Follower Nodes

New nodes join an existing cluster by contacting seed nodes:

**Command Line:**

```bash
# Start a secondary node and join existing cluster
actor-store start \
  --data-dir /var/lib/actor-store-2 \
  --node-name node-secondary-1 \
  --role SECONDARY \
  --rpc-address 0.0.0.0:7002 \
  --http-address 0.0.0.0:8081 \
  --admin-address 0.0.0.0:9091 \
  --join 10.0.1.5:7001

# Or use seed list from config file
actor-store start --config /etc/actor-store/cluster.toml
```

**Join Protocol:**

```rust
fn join_existing_cluster(
    identity: NodeIdentity,
    addresses: NodeAddresses,
    seed_addresses: Vec<SocketAddr>,
) -> Result<Cluster> {
    // 1. Try connecting to seed nodes
    for seed_addr in seed_addresses {
        log::info!("Attempting to join cluster via seed: {}", seed_addr);
        
        match try_join_via_seed(identity.clone(), addresses.clone(), seed_addr) {
            Ok(cluster) => {
                log::info!("Successfully joined cluster: {}", cluster.id);
                return Ok(cluster);
            }
            Err(e) => {
                log::warn!("Failed to join via {}: {}", seed_addr, e);
                continue;
            }
        }
    }
    
    Err(Error::ClusterJoinFailed("All seed nodes unreachable"))
}

fn try_join_via_seed(
    identity: NodeIdentity,
    addresses: NodeAddresses,
    seed_addr: SocketAddr,
) -> Result<Cluster> {
    // 1. Connect to seed node
    let mut client = RpcClient::connect(seed_addr)?;
    
    // 2. Send join request
    let request = JoinRequest {
        node_id: identity.id,
        node_name: identity.name.clone(),
        role: identity.role,
        addresses: addresses.clone(),
        version: identity.version.clone(),
        metadata: identity.metadata.clone(),
    };
    
    let response = client.request_join(request)?;
    
    // 3. Receive cluster information
    let cluster_info = response.cluster_info;
    let current_members = response.members;
    
    // 4. Initialize Raft with full member list
    let raft_config = RaftConfig {
        cluster_id: cluster_info.id.clone(),
        node_id: identity.id,
        initial_members: current_members.clone(),
        election_timeout: Duration::from_millis(300),
        heartbeat_interval: Duration::from_millis(100),
    };
    
    let raft = RaftNode::new(raft_config)?;
    
    // 5. Start as follower (not leader)
    raft.start_as_follower()?;
    
    // 6. Initialize actor store
    let store = ActorStore::new(identity.id)?;
    
    // 7. Request snapshot from leader to catch up
    log::info!("Requesting snapshot from leader to initialize state...");
    let snapshot = client.request_snapshot()?;
    store.restore_from_snapshot(snapshot)?;
    
    // 8. Subscribe to replication stream
    log::info!("Subscribing to replication stream...");
    raft.start_replication_catch_up()?;
    
    // 9. Build cluster state
    let mut members = HashMap::new();
    for member in current_members {
        members.insert(member.node_id, MemberInfo {
            identity: NodeIdentity {
                id: member.node_id,
                name: member.node_name,
                role: member.role,
                version: member.version,
                metadata: member.metadata,
            },
            addresses: member.addresses,
            state: MemberState::ACTIVE,
            last_heartbeat: Instant::now(),
        });
    }
    
    let cluster = Cluster {
        id: cluster_info.id,
        identity,
        addresses,
        raft,
        store,
        members,
    };
    
    // 10. Persist cluster membership
    cluster.save_membership_to_disk()?;
    
    Ok(cluster)
}
```

**What Happens on the Seed Node:**

```rust
// On the receiving (seed) node
async fn handle_join_request(request: JoinRequest, cluster: &Cluster) -> Result<JoinResponse> {
    // 1. Validate join request
    validate_join_request(&request, cluster)?;
    
    // 2. Check if this node is the leader
    if !cluster.raft.is_leader() {
        // Redirect to current leader
        let leader_id = cluster.raft.current_leader()?;
        let leader_addr = cluster.get_member_rpc_address(leader_id)?;
        
        return Ok(JoinResponse::Redirect {
            leader_address: leader_addr,
        });
    }
    
    // 3. Add new member to Raft cluster
    let new_member = Member {
        node_id: request.node_id,
        addresses: request.addresses.clone(),
        role: request.role,
    };
    
    cluster.raft.add_member(new_member)?;
    
    // 4. Wait for membership change to commit
    cluster.raft.wait_for_commit()?;
    
    // 5. Return cluster information
    Ok(JoinResponse::Success {
        cluster_info: ClusterInfo {
            id: cluster.id.clone(),
            name: cluster.name.clone(),
            replication_factor: cluster.replication_factor,
            created_at: cluster.created_at,
        },
        members: cluster.get_all_members(),
        current_leader: cluster.raft.current_leader()?,
    })
}

fn validate_join_request(request: &JoinRequest, cluster: &Cluster) -> Result<()> {
    // Check version compatibility
    if !is_version_compatible(&request.version, &cluster.version) {
        return Err(Error::IncompatibleVersion);
    }
    
    // Check if node already exists
    if cluster.members.contains_key(&request.node_id) {
        return Err(Error::NodeAlreadyExists);
    }
    
    // Check role constraints
    match request.role {
        NodeRole::PRIMARY => {
            // Ensure we don't exceed max PRIMARY nodes
            let primary_count = cluster.count_nodes_by_role(NodeRole::PRIMARY);
            if primary_count >= cluster.max_primary_nodes {
                return Err(Error::TooManyPrimaryNodes);
            }
        }
        NodeRole::SECONDARY | NodeRole::READ_ONLY => {
            // These can be added freely
        }
    }
    
    Ok(())
}
```

### 2.4 Node Registry

The cluster maintains a distributed registry of all member nodes:

```rust
struct MemberRegistry {
    // In-memory view of current membership
    members: HashMap<NodeId, MemberInfo>,
    
    // Persistent storage (replicated via Raft)
    storage: Box<dyn RegistryStorage>,
    
    // Watchers for membership changes
    watchers: Vec<Sender<MembershipEvent>>,
}

struct MemberInfo {
    identity: NodeIdentity,
    addresses: NodeAddresses,
    state: MemberState,
    
    // Health tracking
    last_heartbeat: Instant,
    consecutive_failures: u32,
    
    // Replication tracking
    replication_lag: Option<u64>,  // Sequence number lag
    last_sync: Option<Instant>,
}

enum MemberState {
    JOINING,     // In process of joining cluster
    ACTIVE,      // Fully synchronized and active
    SUSPECTED,   // Heartbeat timeouts, may be failing
    LEAVING,     // Graceful shutdown in progress
    LEFT,        // Gracefully removed from cluster
    FAILED,      // Detected as failed, needs recovery
}
```

**Registry Operations:**

```rust
impl MemberRegistry {
    // Query operations
    fn get_member(&self, node_id: NodeId) -> Option<&MemberInfo>;
    fn get_all_members(&self) -> Vec<MemberInfo>;
    fn get_members_by_role(&self, role: NodeRole) -> Vec<MemberInfo>;
    fn get_active_primary_nodes(&self) -> Vec<MemberInfo>;
    fn get_active_secondary_nodes(&self) -> Vec<MemberInfo>;
    fn get_read_only_nodes(&self) -> Vec<MemberInfo>;
    
    // Modification operations (leader only)
    async fn add_member(&mut self, member: MemberInfo) -> Result<()>;
    async fn remove_member(&mut self, node_id: NodeId) -> Result<()>;
    async fn update_member_state(&mut self, node_id: NodeId, state: MemberState) -> Result<()>;
    async fn promote_to_primary(&mut self, node_id: NodeId) -> Result<()>;
    async fn demote_to_secondary(&mut self, node_id: NodeId) -> Result<()>;
    
    // Watch for changes
    fn watch(&mut self) -> Receiver<MembershipEvent>;
}

enum MembershipEvent {
    MemberJoined(MemberInfo),
    MemberLeft(NodeId),
    MemberFailed(NodeId),
    MemberStateChanged { node_id: NodeId, old_state: MemberState, new_state: MemberState },
    RoleChanged { node_id: NodeId, old_role: NodeRole, new_role: NodeRole },
    LeaderChanged { old_leader: Option<NodeId>, new_leader: NodeId },
}
```

**Registry Persistence:**

The registry is replicated via Raft log entries:

```rust
enum RegistryCommand {
    AddMember { member: MemberInfo },
    RemoveMember { node_id: NodeId },
    UpdateState { node_id: NodeId, state: MemberState },
    PromoteToPrimary { node_id: NodeId },
    DemoteToSecondary { node_id: NodeId },
}

// When leader applies a registry command
async fn apply_registry_command(command: RegistryCommand, registry: &mut MemberRegistry) -> Result<()> {
    match command {
        RegistryCommand::AddMember { member } => {
            registry.members.insert(member.identity.id, member.clone());
            registry.notify_watchers(MembershipEvent::MemberJoined(member));
        }
        RegistryCommand::RemoveMember { node_id } => {
            registry.members.remove(&node_id);
            registry.notify_watchers(MembershipEvent::MemberLeft(node_id));
        }
        // ... other commands
    }
    
    // Persist to storage
    registry.storage.save_snapshot(&registry.members).await?;
    
    Ok(())
}
```

---

## Part 3: Node Roles and Responsibilities

### 3.1 PRIMARY Nodes

PRIMARY nodes are the authoritative writers for the cluster and participate in leader election.

**Capabilities:**

```rust
impl PrimaryNode {
    // Write operations
    async fn handle_create(&self, request: CreateRequest) -> Result<Actor>;
    async fn handle_update(&self, request: UpdateRequest) -> Result<Actor>;
    async fn handle_delete(&self, request: DeleteRequest) -> Result<()>;
    async fn handle_cas(&self, request: CasRequest) -> Result<Actor>;
    
    // Read operations
    async fn handle_read(&self, request: ReadRequest) -> Result<Actor>;
    async fn handle_query(&self, request: QueryRequest) -> Stream<Actor>;
    
    // Leader responsibilities (when elected)
    async fn replicate_to_followers(&self, entry: LogEntry) -> Result<()>;
    async fn maintain_heartbeats(&self) -> Result<()>;
    async fn handle_follower_catch_up(&self, node_id: NodeId) -> Result<()>;
    
    // Raft participation
    async fn participate_in_election(&self) -> Result<()>;
    fn can_vote(&self) -> bool { true }
    fn can_become_leader(&self) -> bool { true }
}
```

**Write Request Handling:**

```rust
async fn handle_write_request(node: &PrimaryNode, request: WriteRequest) -> Result<Response> {
    // 1. Check if this node is the leader
    if !node.raft.is_leader() {
        // Redirect to current leader
        let leader = node.raft.current_leader()?;
        return Err(Error::NotLeader { leader_id: leader });
    }
    
    // 2. Generate operation ID for deduplication
    let op_id = OperationId::generate(&request);
    
    // 3. Check if already executed
    if let Some(result) = node.operation_log.get_completed(op_id) {
        return Ok(result);  // Return cached result
    }
    
    // 4. Validate request
    validate_write_request(&request, &node.store)?;
    
    // 5. Append to Raft log
    let log_entry = LogEntry::Write {
        op_id,
        actor_id: request.actor_id,
        operation: request.operation,
        timestamp: Utc::now(),
    };
    
    let log_index = node.raft.append_entry(log_entry).await?;
    
    // 6. Wait for commit (quorum acknowledgment)
    node.raft.wait_for_commit(log_index).await?;
    
    // 7. Apply to local store
    let result = apply_write(&request, &mut node.store)?;
    
    // 8. Record in operation log
    node.operation_log.mark_completed(op_id, result.clone())?;
    
    // 9. Generate CDC event
    node.cdc.publish_event(ChangeEvent::from_write(&request, &result))?;
    
    Ok(result)
}
```

**Demotion to SECONDARY:**

```rust
async fn demote_to_secondary(node: &mut PrimaryNode) -> Result<SecondaryNode> {
    log::info!("Demoting PRIMARY node {} to SECONDARY", node.identity.id);
    
    // 1. Stop accepting new writes
    node.write_gate.close()?;
    
    // 2. Wait for in-flight writes to complete
    node.wait_for_pending_writes().await?;
    
    // 3. If this node is leader, step down
    if node.raft.is_leader() {
        node.raft.step_down().await?;
    }
    
    // 4. Update role in cluster registry
    node.registry.update_role(node.identity.id, NodeRole::SECONDARY).await?;
    
    // 5. Convert to secondary node
    let secondary = SecondaryNode {
        identity: NodeIdentity {
            role: NodeRole::SECONDARY,
            ..node.identity
        },
        addresses: node.addresses,
        raft: node.raft,
        store: node.store,
        registry: node.registry,
    };
    
    log::info!("Successfully demoted to SECONDARY");
    
    Ok(secondary)
}
```

### 3.2 SECONDARY Nodes

SECONDARY nodes can serve reads and can be promoted to PRIMARY if needed.

**Capabilities:**

```rust
impl SecondaryNode {
    // NO write operations (redirect to leader)
    async fn handle_write_request(&self, request: WriteRequest) -> Result<Response> {
        let leader = self.raft.current_leader()?;
        Err(Error::ReadOnlyReplica { 
            leader_id: leader,
            redirect_address: self.registry.get_member(leader)?.addresses.http_address,
        })
    }
    
    // Read operations (eventual consistency)
    async fn handle_read(&self, request: ReadRequest) -> Result<Actor>;
    async fn handle_query(&self, request: QueryRequest) -> Stream<Actor>;
    
    // Replication follower
    async fn apply_replication(&mut self, entry: LogEntry) -> Result<()>;
    async fn request_snapshot_if_lagging(&mut self) -> Result<()>;
    
    // Raft participation
    async fn participate_in_election(&self) -> Result<()>;
    fn can_vote(&self) -> bool { true }
    fn can_become_leader(&self) -> bool { true }
}
```

**Promotion to PRIMARY:**

```rust
async fn promote_to_primary(node: &mut SecondaryNode) -> Result<PrimaryNode> {
    log::info!("Promoting SECONDARY node {} to PRIMARY", node.identity.id);
    
    // 1. Ensure node is fully caught up
    node.wait_until_caught_up().await?;
    
    // 2. Update role in cluster registry
    node.registry.update_role(node.identity.id, NodeRole::PRIMARY).await?;
    
    // 3. Enable write handling
    let primary = PrimaryNode {
        identity: NodeIdentity {
            role: NodeRole::PRIMARY,
            ..node.identity
        },
        addresses: node.addresses,
        raft: node.raft,
        store: node.store,
        registry: node.registry,
        write_gate: WriteGate::open(),
        operation_log: OperationLog::new(),
    };
    
    // 4. If current leader failed, trigger election
    if !primary.raft.has_active_leader() {
        primary.raft.trigger_election().await?;
    }
    
    log::info!("Successfully promoted to PRIMARY");
    
    Ok(primary)
}
```

### 3.3 READ_ONLY Nodes

READ_ONLY nodes serve only read traffic and never participate in leadership.

**Capabilities:**

```rust
impl ReadOnlyNode {
    // NO write operations
    async fn handle_write_request(&self, request: WriteRequest) -> Result<Response> {
        let leader = self.get_current_leader()?;
        Err(Error::ReadOnlyReplica {
            leader_id: leader,
            redirect_address: self.registry.get_member(leader)?.addresses.http_address,
        })
    }
    
    // Read operations only
    async fn handle_read(&self, request: ReadRequest) -> Result<Actor>;
    async fn handle_query(&self, request: QueryRequest) -> Stream<Actor>;
    
    // Replication follower (no Raft participation)
    async fn apply_replication(&mut self, entry: LogEntry) -> Result<()>;
    
    // NO Raft participation
    fn can_vote(&self) -> bool { false }
    fn can_become_leader(&self) -> bool { false }
}
```

**Key Differences from SECONDARY:**

1. **No Raft Voting**: READ_ONLY nodes do not participate in leader elections or vote on proposals
2. **Simpler Replication**: Uses direct replication stream from PRIMARY, not Raft log
3. **Cannot be Promoted**: Role is permanent unless manually reconfigured and restarted
4. **Lower Consistency**: May lag further behind PRIMARY since not part of quorum

**Use Cases:**

- Cross-region read replicas (high latency, async replication)
- Dedicated analytics/reporting nodes
- Scaling read capacity without affecting write quorum
- Geographic distribution for read locality

**Starting a READ_ONLY Node:**

```bash
actor-store start \
  --data-dir /var/lib/actor-store-readonly \
  --node-name node-readonly-eu \
  --role READ_ONLY \
  --rpc-address 0.0.0.0:7003 \
  --http-address 0.0.0.0:8082 \
  --join 10.0.1.5:7001 \
  --replication-mode async
```

---

## Part 4: Discovery Implementation Examples

### 4.1 DNS-Based Discovery

```rust
async fn discover_via_dns(config: &DnsDiscoveryConfig) -> Result<Vec<NodeAddresses>> {
    let resolver = TokioAsyncResolver::tokio_from_system_conf()?;
    
    // Try SRV records first
    let srv_name = format!("_actor-store._tcp.{}", config.dns_name);
    
    match resolver.srv_lookup(&srv_name).await {
        Ok(srv_records) => {
            let mut nodes = Vec::new();
            
            for srv in srv_records.iter() {
                let hostname = srv.target().to_string();
                let port = srv.port();
                
                // Resolve hostname to IP
                let ips = resolver.lookup_ip(&hostname).await?;
                
                for ip in ips.iter() {
                    nodes.push(NodeAddresses {
                        node_id: NodeId::nil(),  // Will be discovered on connect
                        rpc_address: SocketAddr::new(ip, port),
                        http_address: SocketAddr::new(ip, port + 1000),
                        admin_address: SocketAddr::new(ip, port + 2000),
                        additional_rpc: vec![],
                        additional_http: vec![],
                    });
                }
            }
            
            Ok(nodes)
        }
        Err(_) => {
            // Fall back to A/AAAA records
            let ips = resolver.lookup_ip(&config.dns_name).await?;
            let default_port = config.default_rpc_port.unwrap_or(7001);
            
            let nodes = ips.iter().map(|ip| NodeAddresses {
                node_id: NodeId::nil(),
                rpc_address: SocketAddr::new(ip, default_port),
                http_address: SocketAddr::new(ip, default_port + 1000),
                admin_address: SocketAddr::new(ip, default_port + 2000),
                additional_rpc: vec![],
                additional_http: vec![],
            }).collect();
            
            Ok(nodes)
        }
    }
}
```

### 4.2 Kubernetes Discovery

```rust
async fn discover_via_kubernetes(config: &K8sDiscoveryConfig) -> Result<Vec<NodeAddresses>> {
    let client = kube::Client::try_default().await?;
    let pods: Api<Pod> = Api::namespaced(client, &config.namespace);
    
    // List pods matching label selector
    let lp = ListParams::default()
        .labels(&config.label_selector);
    
    let pod_list = pods.list(&lp).await?;
    
    let mut nodes = Vec::new();
    
    for pod in pod_list.items {
        if let Some(status) = pod.status {
            if let Some(pod_ip) = status.pod_ip {
                let ip: IpAddr = pod_ip.parse()?;
                
                nodes.push(NodeAddresses {
                    node_id: NodeId::nil(),
                    rpc_address: SocketAddr::new(ip, 7001),
                    http_address: SocketAddr::new(ip, 8080),
                    admin_address: SocketAddr::new(ip, 9090),
                    additional_rpc: vec![],
                    additional_http: vec![],
                });
            }
        }
    }
    
    Ok(nodes)
}
```

### 4.3 Consul Discovery

```rust
async fn discover_via_consul(config: &ConsulDiscoveryConfig) -> Result<Vec<NodeAddresses>> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/health/service/{}?passing=true", 
        config.consul_address, 
        config.service_name
    );
    
    let response = client.get(&url).send().await?;
    let services: Vec<ConsulService> = response.json().await?;
    
    let mut nodes = Vec::new();
    
    for service in services {
        let ip: IpAddr = service.Service.Address.parse()?;
        let rpc_port = service.Service.Port;
        
        // Look for additional ports in service metadata
        let http_port = service.Service.Meta
            .get("http_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(rpc_port + 1000);
        
        let admin_port = service.Service.Meta
            .get("admin_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(rpc_port + 2000);
        
        nodes.push(NodeAddresses {
            node_id: NodeId::nil(),
            rpc_address: SocketAddr::new(ip, rpc_port),
            http_address: SocketAddr::new(ip, http_port),
            admin_address: SocketAddr::new(ip, admin_port),
            additional_rpc: vec![],
            additional_http: vec![],
        });
    }
    
    Ok(nodes)
}
```

---

## Part 5: Lifecycle Management

### 5.1 Node Health Monitoring

Each node monitors the health of other nodes through heartbeats:

```rust
struct HealthMonitor {
    registry: Arc<MemberRegistry>,
    heartbeat_interval: Duration,
    failure_threshold: u32,
}

impl HealthMonitor {
    async fn start(&self) {
        let mut interval = tokio::time::interval(self.heartbeat_interval);
        
        loop {
            interval.tick().await;
            self.check_all_nodes().await;
        }
    }
    
    async fn check_all_nodes(&self) {
        let now = Instant::now();
        let timeout = self.heartbeat_interval * 3;
        
        for (node_id, member) in self.registry.members.iter() {
            if *node_id == self.registry.local_node_id() {
                continue;  // Don't check self
            }
            
            let elapsed = now.duration_since(member.last_heartbeat);
            
            if elapsed > timeout {
                // Node suspected
                member.consecutive_failures += 1;
                
                if member.consecutive_failures >= self.failure_threshold {
                    // Declare node as failed
                    log::warn!("Node {} declared FAILED after {} missed heartbeats", 
                        node_id, member.consecutive_failures);
                    
                    self.handle_node_failure(*node_id).await;
                }
            }
        }
    }
    
    async fn handle_node_failure(&self, node_id: NodeId) {
        // Update member state
        self.registry.update_member_state(node_id, MemberState::FAILED).await;
        
        // Trigger appropriate recovery actions
        let member = self.registry.get_member(node_id).unwrap();
        
        match member.identity.role {
            NodeRole::PRIMARY => {
                // If failed node was leader, election will happen automatically
                // If not leader, just mark as unavailable
                log::info!("PRIMARY node {} failed, monitoring for leader election", node_id);
            }
            NodeRole::SECONDARY => {
                // Check if we still have quorum
                let active_primaries = self.registry.get_active_primary_nodes().len();
                let active_secondaries = self.registry.get_active_secondary_nodes().len();
                
                if active_primaries + active_secondaries < self.registry.replication_factor / 2 + 1 {
                    log::error!("Lost quorum! Active voting nodes: {}", 
                        active_primaries + active_secondaries);
                }
            }
            NodeRole::READ_ONLY => {
                // Just mark as unavailable, doesn't affect quorum
                log::info!("READ_ONLY node {} failed", node_id);
            }
        }
    }
}
```

### 5.2 Graceful Shutdown

```rust
async fn graceful_shutdown(node: &mut Node) -> Result<()> {
    log::info!("Initiating graceful shutdown for node {}", node.identity.id);
    
    // 1. Mark as LEAVING in registry
    node.registry.update_member_state(node.identity.id, MemberState::LEAVING).await?;
    
    // 2. Stop accepting new requests
    node.api_server.stop_accepting_connections()?;
    
    // 3. Wait for in-flight requests to complete (with timeout)
    let timeout = Duration::from_secs(30);
    node.wait_for_pending_requests(timeout).await?;
    
    // 4. If this is the leader, step down
    if node.raft.is_leader() {
        log::info!("Stepping down as leader");
        node.raft.step_down().await?;
        
        // Wait for new leader election
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    
    // 5. Flush any pending writes to disk
    node.store.flush_to_disk().await?;
    
    // 6. Close replication connections
    node.raft.close_connections().await?;
    
    // 7. Update state to LEFT
    node.registry.update_member_state(node.identity.id, MemberState::LEFT).await?;
    
    log::info!("Graceful shutdown complete");
    
    Ok(())
}
```

### 5.3 Node Removal

```bash
# Remove a node from the cluster (admin operation)
actor-store admin remove-node \
  --node-id 550e8400-e29b-41d4-a716-446655440000 \
  --cluster-addr 10.0.1.5:9090

# Force remove (for failed nodes that won't come back)
actor-store admin remove-node \
  --node-id 550e8400-e29b-41d4-a716-446655440000 \
  --force
```

```rust
async fn remove_node(cluster: &Cluster, node_id: NodeId, force: bool) -> Result<()> {
    // 1. Check if node exists
    let member = cluster.registry.get_member(node_id)
        .ok_or(Error::NodeNotFound)?;
    
    // 2. If not forced, wait for graceful shutdown
    if !force {
        if member.state != MemberState::LEFT {
            return Err(Error::NodeStillActive);
        }
    }
    
    // 3. Remove from Raft membership
    cluster.raft.remove_member(node_id).await?;
    
    // 4. Remove from registry
    cluster.registry.remove_member(node_id).await?;
    
    // 5. Trigger rebalancing if needed
    if should_rebalance(&cluster) {
        cluster.trigger_rebalance().await?;
    }
    
    Ok(())
}
```

---

## Part 6: Configuration Examples

### 6.1 Single-Node Development Setup

```toml
# dev-single-node.toml

[node]
name = "dev-node-1"
role = "PRIMARY"
data_dir = "/tmp/actor-store-dev"

[cluster]
name = "dev-cluster"
bootstrap = true  # Create new single-node cluster
replication_factor = 1

[network]
rpc_address = "127.0.0.1:7001"
http_address = "127.0.0.1:8080"
admin_address = "127.0.0.1:9090"

[storage]
backend = "memory"  # In-memory only for development
```

### 6.2 Three-Node Production Cluster

**Node 1 (Primary):**
```toml
# production-node-1.toml

[node]
name = "prod-primary-1"
role = "PRIMARY"
data_dir = "/var/lib/actor-store"

[cluster]
name = "production-cluster"
bootstrap = true
replication_factor = 3

[network]
rpc_address = "10.0.1.5:7001"
http_address = "10.0.1.5:8080"
admin_address = "10.0.1.5:9090"

[storage]
backend = "rocksdb"
rocksdb_path = "/var/lib/actor-store/data"

[discovery]
method = "static"

[[discovery.seeds]]
node_id = "550e8400-e29b-41d4-a716-446655440000"
rpc_address = "10.0.1.5:7001"

[[discovery.seeds]]
node_id = "7c9e6679-7425-40de-944b-e07fc1f90ae7"
rpc_address = "10.0.1.6:7001"

[[discovery.seeds]]
node_id = "9f8e3e95-7ae7-4f8e-9e3e-7f8e9e3e7f8e"
rpc_address = "10.0.1.7:7001"
```

**Node 2 (Secondary):**
```toml
# production-node-2.toml

[node]
name = "prod-secondary-1"
role = "SECONDARY"
data_dir = "/var/lib/actor-store"

[cluster]
name = "production-cluster"

[network]
rpc_address = "10.0.1.6:7001"
http_address = "10.0.1.6:8080"
admin_address = "10.0.1.6:9090"

[storage]
backend = "rocksdb"
rocksdb_path = "/var/lib/actor-store/data"

[discovery]
method = "static"

[[discovery.seeds]]
node_id = "550e8400-e29b-41d4-a716-446655440000"
rpc_address = "10.0.1.5:7001"
```

**Node 3 (Secondary):**
```toml
# production-node-3.toml

[node]
name = "prod-secondary-2"
role = "SECONDARY"
data_dir = "/var/lib/actor-store"

[cluster]
name = "production-cluster"

[network]
rpc_address = "10.0.1.7:7001"
http_address = "10.0.1.7:8080"
admin_address = "10.0.1.7:9090"

[storage]
backend = "rocksdb"
rocksdb_path = "/var/lib/actor-store/data"

[discovery]
method = "static"

[[discovery.seeds]]
node_id = "550e8400-e29b-41d4-a716-446655440000"
rpc_address = "10.0.1.5:7001"
```

### 6.3 Multi-Region with Read Replicas

```toml
# us-east-primary.toml (PRIMARY in US East)
[node]
name = "us-east-primary"
role = "PRIMARY"

[network]
rpc_address = "10.0.1.5:7001"
http_address = "10.0.1.5:8080"

# ... standard config ...

# eu-west-readonly.toml (READ_ONLY in EU West)
[node]
name = "eu-west-readonly"
role = "READ_ONLY"

[network]
rpc_address = "10.1.1.5:7001"
http_address = "10.1.1.5:8080"

[replication]
mode = "async"  # Async replication for cross-region
lag_warning_threshold = 10000  # Warn if 10k operations behind

[discovery]
method = "static"
[[discovery.seeds]]
rpc_address = "10.0.1.5:7001"  # Connect to US East primary
```

---

## Summary

This document completes the Actor Store architecture with:

1. **Node Identity System**: UUID-based node IDs separate from network addresses, enabling mobility and multi-protocol support
2. **Discovery Mechanisms**: Multiple bootstrap methods including static, DNS, Kubernetes, and service registries
3. **Node Roles**: Clear distinction between PRIMARY (write + read + voting), SECONDARY (read + voting), and READ_ONLY (read only, no voting)
4. **Cluster Formation**: Step-by-step protocols for single-node bootstrap and follower join operations
5. **Member Registry**: Distributed registry of all nodes with health tracking and role management
6. **Lifecycle Management**: Health monitoring, graceful shutdown, and node removal procedures
7. **Configuration Examples**: Real-world configurations for development, production, and multi-region deployments

The architecture provides a complete foundation for building the distributed Actor Store with clear operational procedures for cluster management.
