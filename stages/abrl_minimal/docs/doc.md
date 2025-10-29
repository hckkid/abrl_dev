# Goal

ABRL is Actor based Reactive Language. A very basic implementation can be as a language/library that allows creation of Actors,
defining fields on Actors, ability to define condition on changes to fields (Hierarchical, in case of records one can listen to sub element change, in case of Map maybe changes to specific key? in case of array maybe to specific item?). While a minimal approach can be to have in-memory implementation of ABRL, where each Actor has its object, its fields are reactive and reactive condition cause triggers. There are few things such minimal approach doesn't address which are core to a minimal ABRL, 1. The Actor Store can be Distributed and with replication. 2. ABRL deployment itself can be distributed and replicated.

## Few Directions

1. Make ABRL work with language native objects (i.e. Plain old rust objects)
   1. Changes captured on these can only be picked by local ABRL runtime.
2. Make ABRL Have an in memory store, replicated across multiple runtimes.
   1. Changes captured on these can be picked by any ABRL runtime, as long as change and latest copy is reflected on the runtime picking the change.
   2. We will have 2 choices here, either we do latest copy based ops like that in redis, or document level distributed lock like mongodb.
3. Make ABRL work with a Mongo+Change Stream(either Mongo Change Stream or Kafka) based store.
   1. Changes captured on these can be picked by any ABRL runtime, changes are captured in streaming manner, and data is fetched from Store.
   2. Need to figure out how I can make only one worker pick a change, without doing contention based locking.

## Syntax

### Example

This example assumes a minimal implementation, where every Actor is stored as triplet, Key, Value and Version.
Similar to Redis Key Value Store. Versioning is maintained by the store itself. For now key will also
be ObjectId, and auto generated at Insertion, programmer will have to keep track of it.
Also Actor store will allow finding actors based on condition on Value (Later to be made smarter).
To make it smarter we will need to allow field definitions along with indexing. Which will drastically increase scope of in memory store.
For Store like Mongo, we can allow escape hatch of specifying filter condition in plain text for time being.

```text
actor User {
    // id: ObjectID, implicitly created & deleted never updated.
    // value: Value, implicitly created / replaced / deleted.
    // version: number, implicitly created / increments atomically on every update.
    // id, value and version are reserved fields with predefined types and meaning.
    
    // self will always be ActorObject { id: string, value: Value, version: number }
    // Store is defined for each Actor Type, i.e. Store<User>, Store<Order> etc. Self is used to mean current Actor Type.
    action update_name(self, new_name: string) {
        // updates the name field in value.
        self.value.name = new_name;
        Store<Self>.update(self.id, self.value);
    }
    
    action print_name(self) {
        print(self.value.name);
    }
    
    @api("/update_name/:id/:new_name")
    trigger UpdateNameApi
    
    @schedule("0 */1 * * * *")
    trigger ScheduledTrigger
    
    trigger DataChangeTrigger when self.value!!
    
    rule OnNameUpdate (DataChangeTrigger && self.value.name == "Alice") => print_name;
    
    rule OnApiCall (UpdateNameApi) => update_name;
    
    // Since it applies to all actor instances, every minute it will print name for all actor instances.
    rule OnSchedule (ScheduledTrigger) => print_name;
    
    rule OnScheduledAlice (ScheduledTrigger && self.value.name == "Alice") => print_name;
}
```

## Existing state of art

### Signals

```text
let [signal, setSignal] = useSignal(0);
let [signal, setSignal] = useSignal(() => props.name);
```

# TODOS

- Ability for condition to specify whether to apply on past events meeting the condition or only future events.
- Ability to specify whether condition check is temporal and time gated, for example, lets say a match formed notification has to be sent immediately, what if by the time we want to trigger event, recipient has paused the comms. Now if in future they enable the comm, condition becomes true and since rule has been enabled in past skip old events flag won't stop it, so we need a separate condition to gate these events to be sent within certain window. I think its easy with time based check against some timestamp field capturing the time of change.

Lets think of something small,

Say Actors are Always String -> Value(JSON like). i.e. Actor always has a string identifier and its contents are JSON like.
So every Actor becomes like below

```text
actor SampleActor {
  id: string,
  data: value,
  version: number
}
```

This is forward compatible as we introduce more types and allow for arbitrary fields on Actors later on.
But this representation is sufficient for us to build reactivity primitives.

# Minimal Cut

1. Every actor has id, data and version field.
   1. id is always string (We can use mongodb ObjectId, gives us nice \[u8;12\] representation)
   2. version is always number (Rust u64)
   3. Value will be JSON like, for starting we can use serde_json::Value
2. Every Actor Store has following methods (per Actor Type):
   1. insert(data: Value) -> ActorData // store id as required.
   2. get(id: string) -> Option<ActorData>
   3. update(id: string, data: Value) -> ActorData // increments version always
   4. update(id: string, data: Value, expected_version: number) -> Result<ActorData, VersionMismatchError> // increments version always
   5. delete(id: string) -> Option<ActorData>
   6. find(condition: Condition) -> Vec<ActorData> // Condition is fn Value -> bool.
   7. all() -> Vec<ActorData>
3. Every Actor Store provides a way to listen to changes, with resumability.
4. There are Distributed Change subscribers, i.e. multiple schedulers subscribe to changes, but execution is only processed exactly once.
   1. Actor id is used as partition key (maintain in order processing per actor).
      1. If processing a change, itself causes change to Actor, that change will be processed in its own time.

# Architecture

1. Actor Store
   1. Needs leader election as well as failure detection.
   2. Needs rebalancing implementation.
   3. Conflict Resolution, in case writes can happen in parallel to different nodes, instead of replication based.
2. Schedulers
3. Executors

Runtime can spawn one or multiple of above in same process.