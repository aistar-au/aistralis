# ADR-015: Local Endpoint Text-Protocol Default for Tool Loop Reliability

**Date:** 2026-02-21
**Status:** Accepted
**Deciders:** Core maintainer
**Related tasks:** `TASKS/completed/CRIT-18-local-tool-loop-enrichment-consistency.md`

## Context

Local model runs showed unstable behavior when structured tool protocol was enabled by
default:

- local assistants often narrated intent without issuing valid tool calls
- some rounds failed to consume prior `tool_result` context
- protocol mismatch at local endpoints caused noisy, low-signal loops

The runtime contract requires iterative tool execution with enrichment carried across
rounds. Local endpoints need a safer default that preserves this loop.

## Decision

1. Default `VEX_STRUCTURED_TOOL_PROTOCOL` to `false` for local endpoints when the
   env var is unset.
2. Keep remote endpoints defaulting to structured tool protocol (`true`).
3. Keep explicit env override support:
   - `VEX_STRUCTURED_TOOL_PROTOCOL=on|off` always wins.
4. Preserve text-protocol fallback loop behavior as the canonical local reliability
   path:
   - assistant tool calls persisted as rendered tagged text
   - tool results appended as user text payload for next-round enrichment

## Rationale

- Matches common local-server capability (text flow, not reliable structured tools).
- Improves loop completion odds for file-system fact queries requiring tools.
- Keeps remote provider behavior unchanged.
- Maintains runtime-core ownership of loop policy and avoids UI-level workarounds.

## Alternatives considered

1. Keep structured protocol default on for local endpoints
   - Rejected: repeated field reports of failed tool-loop enrichment.
2. Disable structured protocol globally
   - Rejected: weakens remote tool-call quality.
3. Add UI-layer retries only
   - Rejected: violates runtime-core canonical loop ownership.

## Consequences

- Local endpoints run in a more conservative, robust tool loop by default.
- Advanced local setups can re-enable structured protocol explicitly.
- Tests must cover local default/off behavior and text-protocol enrichment continuity.

## Compliance notes for agents

1. Do not route local loop reliability fixes through TUI-only logic.
2. Keep loop enrichment guarantees validated in `src/state/conversation.rs` tests.
3. Keep protocol default decisions inside runtime API client construction.
