# ADR-051: Coordinate Environment lifecycle and expose the same scoped control service to extensions

- Decision date: 2026-09-25
- Status: Accepted
- Amends: [ADR-038](2026-09-05-07-roadmap-boundary.md), [ADR-041](2026-09-05-10-one-execution-model.md), [ADR-046](2026-09-09-01-minimal-extension-contract.md), [ADR-048](2026-09-21-01-tool-completion-and-event-activation.md)

## Context

An agent should keep its conversation and diagnostic capability when a disposable sandbox
fails. Existing execution receipts describe calls, but do not provide idle resource observations,
explicit setup policy or model-directed control of authorized Environment instances.

Applications also need to manage resources inside an Environment. Those resources vary:
browser tabs, jobs and containers have different identities and operations. Encoding them all
in Brain would expand the kernel without giving it the knowledge to manage them correctly.

The design follows the minimal extension approach of [Pi](https://pi.dev/) and the separation
of session, harness and sandbox described by [Anthropic](https://www.anthropic.com/engineering/managed-agents).
Brain remains a durable session engine with replaceable Agentloop, Tool and Env implementations.

## Decision

### Ownership

| Owner | Responsibility |
| --- | --- |
| Brain | Commit operation intents and outcomes, enforce fixed authority, project binding state, pin incarnations and coordinate lifecycle. Report failures at the transport boundary. |
| Agentloop | Own transcript and KV; select model context, Tool placement and recovery policy. |
| Tool | Perform its task using its own granted services; report results and finish. |
| Env | Implement lifecycle effects, execution, provider inspection, resource operations and monitoring. |
| Application | Choose explicit lifecycle policy, templates, schemas, grants and installed Tools. |
| Hosted platform | Select provider offers and enforce customer authorization, quotas and billing. |

The journal remains the only durable session truth. There is no second event queue, generic
resource ownership tree, cloud SDK in the kernel or automatic recovery policy.

### Explicit lifecycle and ordinary Tool composition

Every newly created binding has `automatic` or `manual` lifecycle. The SDK requires
`environmentLifecycle: { default, bindings? }`; this is separate from opaque provider options.
Automatic bindings run setup at declaration. Manual bindings are discoverable but cannot execute
Tools until explicitly set up. The Agentloop's bootstrap binding must be automatic.

Both policies retain caller cleanup: detach at session end and teardown at deletion for bindings
whose setup was attempted. An untouched declaration needs no remote cleanup. Resource retention
and allocation remain Env semantics. No failed or uncertain setup is implicitly repeated.

The application independently imports and includes the official `env` Tool, or any custom Tool.
Automatic lifecycle does not install it. Removing or renaming it changes no lifecycle behavior.
Fully managed, diagnostics-enabled and model-managed setup are compositions of these choices,
not special kernel modes. Management code must run outside an Environment it can replace.

### One extension service

Agentloop, Tool and Env operation contexts use the same `environments` service:
`list`, `get`, `create`, `update`, `setup`, `delete` and `call`. Each role has its own explicit
grants, naming an admitted binding/template, permitted operations and permitted method names.
The official Tool uses only this public interface. There is no Tool-name check or privileged path.

`get` reads recorded state. Live inspection is an Env-defined `call(reference, "inspect", input)`.
There is no official `env.inspect`. Named methods declare input/output schemas and whether
they replace the entire binding (`effect: "replace"`). Resource CRUD stays behind these methods.

Shared authoring vocabulary includes options, operation identity, cancellation, emission and
Environment services. Real ownership differences remain explicit: only the Agentloop writes the
transcript/KV; a Tool owns its results and completion; an Env owns provider/resource observations.
An Env controller never inherits the model or Tool callback authority of code it executes.

### Fixed authority, dynamic state

Templates are admitted with a configuration schema and maximum instance count, including the
original binding. Instances inherit the source driver, sealed credential, methods, lifecycle,
controller grants and authorized Tool placements. A model cannot change these ceilings, register
new executable code or choose a new endpoint. Declared configuration can change only before
the first setup attempt and only within its admitted schema.

Bindings have `{ name, sequence }` references; the sequence is their declaration or replacement
record. Tool admission pins that incarnation. Creation or deletion affects subsequent calls,
including within a turn, without redirecting in-flight calls. Activation Tool metadata includes
references; authorized loops can refresh live instances through the shared service.

Whole-binding replacement requires quiescent execution and cannot replace the caller's own
environment or the Agentloop bootstrap environment. Deletion fences new execution, settles
active Tool cancellation and then sends teardown. This does not claim rollback of effects.

### Observations and lifetime

An Env has two capabilities. Its operation context closes on return/cancellation. A separate
binding reporter may remain with the controller to report changes while no Tool is running.
The reporter grants only bounded diagnostic emission and typed observations, never session
control, model calls, transcript or KV access. Tokens are sealed outside the journal and scoped
to session, binding and incarnation. Detach, deletion and replacement invalidate stale reports.

Brain attributes observations and commits them before acknowledgement. Environment availability,
individual resource failure and resolution of a named lifecycle operation are distinct facts.
A resource event cannot clear a binding's unavailable state. Diagnostic progress does not wake
the loop; actionable observations use the existing coalesced activation path and journal watermark.
Only the Agentloop decides how to include them in model context.

Events arriving during inference are available on a later observation pass; they cannot alter
an in-flight model request. Interruption suppresses automatic activation until an explicit new
message. Startup loads history without replaying effects or old activations.

Transport failure, Env-reported failure and an unknown effect remain distinguishable. Retryability
is advisory. Unknown lifecycle outcomes keep capacity reserved; only authenticated evidence naming
the pending operation can settle them. Old operation results cannot overwrite a newer incarnation
or a confirmed resolution. Brain never infers that a failed response means no effect occurred.

### Compatibility and placement

Existing journals without explicit lifecycle retain their original automatic semantics. New
admission requires a policy. Existing application-level Environment method routes remain usable
for retained clients; extension methods always pass through their fixed grants and schemas.
The control coordinator belongs to `brain-sessions`; the core depends on its public port.
SDK, HTTP and WIT bindings are generated or implemented from the same Rust service contract.

## Alternatives and tradeoffs

- A built-in privileged management Tool would be shorter initially but make third-party
  implementations unequal. An ordinary Tool over a shared service keeps extension policy external.
- A universal inspect/restart/resource API would standardize names while hiding provider-specific
  behavior. Cached `get` plus declared Env methods preserves the meaningful common boundary.
- Model-only setup couples task success to housekeeping. Explicit lifecycle policy supports
  automatic setup without granting repair authority or injecting tools.
- Arbitrary runtime registration or mutable permissions would weaken admission. Templates allow
  useful instance changes while retaining the original authority ceiling.
- Holding a session lock during remote control would deadlock same-turn and cross-Env calls.
  Short admission locks protect state transitions; remote effects run after durable intent.
- Automatic retries, polling and healing require provider-specific safety decisions. Brain records
  uncertainty and lets authorized application/extension policy choose the next action.

The cost is an additional scoped capability and journal projection. It is shared across extension
roles and reuses existing delivery, lifetime and activation mechanisms. No new workflow engine
or scheduler is introduced.

## Verification

Control tests cover admission ceilings, manual readiness, stale references, uncertain operation
resolution after reopen, observation bounds and resource/availability separation. Session tests
exercise the same service from all three extension roles, same-turn setup/dispatch, idle/busy
observations, watermark consumption and interruption. SDK tests cover authoring types and expired
contexts; HTTP journeys verify retained reporters through a server restart. Required CI includes
real worker, image and provider gates. Public behavior lives in the
[control guide](../../docs/guides/environment-control.mdx) and generated contracts.
