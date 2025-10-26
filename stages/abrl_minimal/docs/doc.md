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
  data: value
}
```

This is forward compatible as we introduce more types and allow for arbitrary fields on Actors later on.
But this representation is sufficient for us to build reactivity primitives.

