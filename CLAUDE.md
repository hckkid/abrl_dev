# Structure


Referenced below is a guiding project structure, as such the overall project is more of workspace and each crate inside stages is actual project. Instead of tracking self-sufficient versions of ABRL with progressive feature development through branches on git, I have decided to instead have full crate with desired capabilities starting with minimal version.
```text
abrl-dev/
├─ Cargo.toml                # workspace root
├─ Justfile                  # or Makefile: helper tasks
├─ stages/
│  ├─ abrl_minimal/
│  │  ├─ Cargo.toml
│  │  └─ src/…
│  ├─ 01_lexer/
│  │  ├─ Cargo.toml
│  │  └─ src/…
│  ├─ 02_parser/
│  ├─ 03_ast_eval/
│  ├─ 04_reactive_vars/
│  ├─ 05_scheduler_tick/
│  ├─ 06_event_bus/
│  ├─ 07_actor_ids_mailbox/
│  ├─ 08_transactions_basic/
│  ├─ 09_transactions_conflicts/
│  ├─ 10_time_triggers/
│  ├─ 11_persistence_snapshot/
│  ├─ 12_dedup_throttle/
│  ├─ 13_distributed_proto/
│  └─ … (keep going)
├─ shared/
│  ├─ abrl_testing/          # test harness crate reused by all stages
│  ├─ sample_programs/       # golden DSL files used across stages
│  └─ fixtures/
├─ docs/
│  ├─ book/                  # mdBook (one chapter per stage)
│  └─ stage_guides/
└─ tools/
├─ stagegen/              # small CLI to create next stage from previous
├─ stagediff              # script: show diff between stages
└─ check.sh               # runs tests across all stages
```