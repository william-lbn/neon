# Dynamic shared buffers and elastic compute

Status: implementation in progress; disabled by default. This RFC supersedes
the PG18 assumptions in the initial DSGA proposal. An implementation or a passing
build alone does not enable a feature in a released compute image.

## Scope and release contract

The project covers dynamic shared buffers, safe coordination with compute memory,
upstream maintenance, and the integration contracts for horizontal read scaling,
migration, suspend/resume, and HA. These capabilities have separate release gates.
Buffer resizing must not change the authority of WAL or the pageserver.

| Capability | Implementation contract | Release gate |
| --- | --- | --- |
| Buffer grow/shrink | Stable addresses, separate allocation/access bounds, normal eviction | PG concurrency and recovery tests |
| CPU/guest RAM scaling | Existing NeonVM/agent coordination with actual completion | VM resource reconciliation |
| HugeTLB | Explicit backing capabilities, pool/reservation accounting | Real KVM/guest/host tests |
| Automatic policy | Load/WSS/latency budgets, cooldown, admission and rate limits | Workload and cost comparison |
| Independent RW/RO sizing | Per-endpoint policy and limits | Primary/replica integration |
| Horizontal RO scaling | Create, catch up, warm, route, drain, delete | Replica lifecycle and routing tests |
| Migration | One active resource operation, stable VM/network identity | Live migration and failure tests |
| Suspend/resume | Fence old instance; rebuild buffers under current budget | Lifecycle and first-query tests |
| HA | WAL quorum, compute fencing, placement and storage recovery | Independent failure-domain exercises |
| Other DSGA components | A component-specific proof and adapter | A separate reviewed implementation |

GitHub CI is the current build/test environment. Real Linux VM/KVM/HugeTLB
validation is deferred by the owner. Features requiring those tests remain off
until that gate passes; the implementation must report unsupported capability.

## Source and upgrade baseline

Use the existing integrated PG17 first. PG18 requires a separate main-repository,
extension and image compatibility change; a postgres branch is not distribution
support. Keep PG14-16 behavior unchanged except for separately validated upstream
maintenance. Do not transplant dynamic-buffer code into every major at once.

The source of truth is a distribution manifest containing:

- Neon, Autoscaling and Postgres repository/commit identities;
- every PG gitlink and the matching `ci/distribution.json` entry;
- PostgreSQL upstream stable branch/commit and Neon fork base;
- monitor protocol/capability version and image digests;
- build flags, architecture, compiler, guest kernel and QEMU version.

Integrate PostgreSQL stable fixes while preserving Neon changes. Upstream fixes
that affect WAL require decoder and redo compatibility before compute rollout.
Deploy readers first, then writers. Old WAL remains readable across the upgrade.

## Required invariants

Let L be the allocation limit, H the initialized/access limit, and M the reserved
maximum for one postmaster lifetime. In steady state, `0 < L = H <= M`. During
shrink, `0 < L < H <= M` is allowed. M is a startup limit; exceeding it requires a
controlled compute restart or a later separately implemented region extension.

1. New victim allocation uses [0,L). Existing pins in [L,H) remain valid.
2. Every descriptor/block read is protected by a pin or the array read-side lock.
   An atomic H snapshot alone does not protect a subsequent dereference.
3. A tag remains unique. Reclamation rechecks refcount, dirty, IO and tag state
   under the normal mapping/header lock protocol; it does not force-clear pins.
4. The final tag removal is serialized with hash-hit pin acquisition. A busy
   page can deny shrink without disrupting queries.
5. Recent-buffer hints and strategy rings are validated before dereferencing a
   descriptor. Shrink followed by grow must not resurrect stale page identity.
6. No backing is removed until pins/IO and old array visitors have drained and
   future visitors can only acquire the new H.
7. New backends and workers observe the published state before using buffers;
   exiting participants are handled by existing PG process-slot semantics.
8. Normal PG/Neon WAL, smgr and eviction rules perform dirty-page processing.
9. Logical commit and physical release are separate facts. Failure can retain
   extra resources, but must never authorize release of still-live resources.
10. A resize permission is tied to VM UID, compute instance, PG boot identity
    and operation revision; it is invalid after any of these identities changes.

The coordinator must not wait for a barrier while holding a lock needed by a
participant to acknowledge it. Lock order, error cleanup and interrupt safety
are part of the code review, not merely properties of the state diagram.

## Initial memory layout

Start with Linux fork and stable shared virtual ranges, committing only the
active portion. Keep backing-object identity and addresses across resize. Avoid
runtime FD distribution and per-backend new mappings in the first provider.
`mprotect` and `munmap` are process-local operations; inheritance does not make
future mapping changes appear in existing children.

Buffer blocks, descriptors, IO condition variables and checkpoint workspace need
explicit per-array rules. Keep the tag hash sized to M initially; account for its
minimum-spec overhead. Control metadata and locks are never reclaimed with a
buffer granule. Round reclaim ranges inward to safe provider page boundaries;
never discard an active object sharing the boundary page.

DSGA is initially a small provider/coordinator interface. Do not introduce a
general shared allocator or independently restartable MMAN process before buffer
correctness is established. A second component must define its own references,
quiescence and recovery; an adapter does not make it resizable automatically.

## Operation identity and observable state

The monitor must negotiate the capability before enabling dynamic memory. Keep
the existing agent Revision as the outer ordering mechanism. The PG client adds
boot identity and a client-generated operation ID; replay of a request returns
its known result rather than executing it twice. A changed target is a new
operation, not reuse of an old ID.

Report target bytes, L/H/M, physically committed bytes, unused committed bytes,
release pending bytes, provider/page size, operation identity, phase and failure
reason. `SHOW shared_buffers` remains a parseable size. Target and effective
capacity are exposed separately. SQL controls are restricted to an internal
administrator and enforce range/size checks inside the function.

States include IDLE, PREPARING, DRAINING, LOGICAL_COMMITTED,
PHYSICAL_PENDING, COMMITTED, DENIED and UNKNOWN. UNKNOWN prohibits VM shrink.
Cancellation before logical commit can restore the old allocation boundary if
backing remains intact. After physical release it cannot blindly restore ACTIVE;
restoration is a new grow. All waits are interruptible and bounded by policy.

## Grow protocol

1. Check identities, revision, quota, M and total target memory budget.
2. Reserve scheduler resources, increase the VM, and observe guest availability.
3. Commit/prepopulate new block and metadata ranges. Track partial allocation on
   each provider call, including failures.
4. Initialize every new object before making it visible.
5. Under publication protection, increase H before L. Normal readers see either
   the old pool or initialized objects in the new pool.
6. Return actual state; reconcile late/missing replies by operation identity.

A PG failure after VM grow leaves a larger VM until safe reconciliation. A
failed prepopulate must not publish the target or hide unused committed memory.

## Shrink protocol

1. Check the target budget and serialize the operation; reject migration overlap.
2. Lower L while retaining H; synchronize old allocation/ring users.
3. Evict tail pages through the standard pin/tag/dirty/IO protocol, with bounded
   work batches and retry. New pins can postpone or deny the operation.
4. Exclude new old-range array visitors and drain existing visitors; invalidate
   stale hints as required. Publish smaller H under the access lock.
5. Release safe physical ranges. Record partial success without rolling back to
   descriptors whose backing no longer exists.
6. Adjust guest HugeTLB reservations/pool if supported. Maintain the target
   query-memory budget while the outer shrink is pending.
7. Authorize only the current operation/boot/revision. The final VM mutation
   verifies fencing; observe virtio-mem completion rather than spec equality.
8. Release scheduler commitments only after actual resource reconciliation.

PG can be small while the VM remains large. This is a safe pending state and
must be observable. A prior permission cannot authorize shrink after a newer
grow. Restart chooses startup shared buffers from current actual guest budget,
not a stale larger `postgresql.auto.conf` value.

## Memory budget and policy

Budget OS/kernel/normal-zone memory, page tables, other shared memory, actual
buffer commitment, active query and parallel-worker memory, maintenance, LFC
metadata/resident working set, unreleased HugeTLB pool, transient resize overhead
and margin. Deduplicate shared memory and HugeTLB accounting. Do not sum backend
RSS or multiply `work_mem * max_connections` as if it were an exact bound.

Grow can be triggered by sustained CPU or memory demand, but distinguish IO/lock
bottlenecks. Shrink requires sustained headroom across all budgets, a safe PG
result and acceptable latency. Use separate grow/shrink windows, asymmetric
cooldown, step limits, emergency grow priority, resize-rate limits and maximum
pending duration. Report inability to reach the minimum rather than forcing it.

Validate WSS estimator inputs after more reads hit shared buffers instead of the
LFC. A missing LFC event must not be interpreted as proof that the working set
disappeared. Thresholds and the buffer/guest-memory ratio are workload settings,
not hard-coded copies of the PolarDB examples.

## HugeTLB and VM provider contract

The current runner uses `memory-backend-ram` and virtio-mem with 8 MiB blocks.
Guest VA granules do not correspond one-for-one with unplugged guest PFNs or
host pages. PG proves object quiescence; the guest kernel/virtio-mem proves
physical migration/unplug; the host reconciles backing and resource commitments.

Distinguish VA reservation, HugeTLB reservation, allocated pages, free pool pages,
surplus pages, guest managed memory and host backing. Returning pages to the
guest pool is not host release. Check provider advice support, page alignment,
NUMA placement, memory onlining/zone balance, cgroup accounting and migration
compatibility. Unsupported or uncertain states deny physical shrink.

## Horizontal read scaling contract

A group has exactly one writable endpoint identity and separately managed RO
endpoints on the same branch/timeline under explicit consistency policy. Scale
out follows REQUESTED -> PROVISIONING -> CATCHING_UP -> WARMING -> READY ->
ROUTED. A bounded lease prevents several agents provisioning the same capacity.
The controller must verify replica role, health and required LSN before routing.

Scale in first removes the RO from new connection selection, drains existing
sessions with a deadline/policy, then stops/deletes it. It must never select the
RW endpoint. Group min/max, per-node vertical max and total budget constrain the
decision. Low lag alone is not readiness; failed provisioning is reconciled.

Read/write routing uses explicit endpoints or a separately validated proxy
policy. Do not guess that arbitrary SQL is read-only or migrate transactions
between backends. Read-after-write requires primary affinity or an LSN token
and wait policy. Session state, temporary objects and transactions remain bound
to the selected connection unless an explicit pooling contract says otherwise.

The provisioning/routing API must bind to the deployment's actual control plane;
the development `neon_local` CLI is not an implicit production orchestrator.

## Migration, suspension and HA contracts

Migration and resize share an operation lease and fencing revision. Pause new
resizes, reconcile PG and VM actual state, validate target capacity/backing,
perform migration, verify actual completion and release the lease. VM network
identity and transport state must survive for connection-preservation claims.
Crash/recreate failover is a different availability event and may reconnect.

Suspend stops new connection admission, drains or applies the configured session
policy, confirms WAL durability under existing Neon rules, and fences the old
instance before releasing resources. Resume builds a new buffer pool from its
current budget and advertises readiness only after role/storage checks. Buffers
and raw virtual addresses are not durable state.

HA must separately validate safekeeper quorum, writable-compute fencing,
pageserver/controller recovery and independent placement. Three VMs on one
physical host do not prove physical fault tolerance. RPO/RTO and interruption
budgets must be declared by deployment; there is no universal zero-loss or
zero-interruption promise from this RFC.

## Tests and release gates

Required deterministic cases: hash-hit re-pin during drain; tagged-but-not-valid
IO; scanner paused with old H; backend join/exit during barrier; stale recent ID
and ring across shrink/regrow; partial prepopulate/remove per array; interrupt
and crash at each commit boundary; PG restart after VM shrink; old-agent delayed
permission; private-memory burst while VM shrink is pending; legacy/new WAL VM
references including shared and distinct old/new VM pages; FSM/VM, UNLOGGED,
index build, logical decoding and RO replay.

Use real test pins/injection points; an idle transaction is not a pinned-tail
test. Assertions, checksums, amcheck and recovery/result oracles complement
stress/soak tests. Compare static small buffer+LFC, static large buffer, dynamic
normal pages and dynamic HugeTLB under equal actual resources.

Gates: (A) updated baseline and recovery; (B) PG normal-page concurrency;
(C) Neon storage/decoder integration; (D) monitor/agent identity and budgets;
(E) actual VM/hugetlb tests; (F) canary, rollback and workload/cost validation.
Gates E/F remain pending until a real environment is supplied and tested.

## Upstream maintenance

Track Neon main, Autoscaling main and each supported Neon PG stable branch.
Also track the corresponding PostgreSQL stable branch and security advisories.
Fetch/compare can be scheduled; changes enter reviewed PRs and appropriate CI,
not automatic production merges. Use commit IDs, never mutable `latest` inputs.

A sync PR records bases, upstream heads, conflict resolutions, ABI/WAL/protocol
impact, tests and rollout order. Update PG gitlink and distribution lock together.
Preserve the fork's publishing policy when upstream changes workflow paths.
Keep feature patches separate from upstream merges so future conflict review is
bounded. Experimental community buffer patches are algorithm/test inputs, not
stable maintenance releases.
