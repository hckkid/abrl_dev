# Goal

ABRL is Actor based Reactive Language. A very basic implementation can be as a language/library that allows creation of Actors,
defining fields on Actors, ability to define condition on changes to fields (Hierarchical, in case of records one can listen to sub element change, in case of Map maybe changes to specific key? in case of array maybe to specific item?). While a minimal approach can be to have in-memory implementation of ABRL, where each Actor has its object, its fields are reactive and reactive condition cause triggers. There are few things such minimal approach doesn't address which are core to a minimal ABRL, 1. The Actor Store can be Distributed and with replication. 2. ABRL deployment itself can be distributed and replicated.

## Existing state of art

```text
let [signal, setSignal] = useSignal(0);
let [signal, setSignal] = useSignal(() => props.name);
```