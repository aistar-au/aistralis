# ADR-002: Lexical path normalization over `fs::canonicalize()` in tool executor

**Date:** 2026-02-18  
**Status:** Accepted  
**Deciders:** Core maintainer  
**Related tasks:** `TASKS/SEC-01-path-security.md`  
**Implemented in:** `src/tools/executor.rs` — `resolve_path()`, `normalize_path()`

---

## Context

`vexcoder` gives an LLM direct `write_file` and `edit_file` access to the working directory. The tool executor must prevent the model from writing outside the workspace — whether through `..` traversal, absolute paths, or symlink escapes.

The naive solution is to call `std::fs::canonicalize()` on the resolved path and verify it starts with the canonical working directory. This is a well-known pattern and is correct for existing files.

However, the tool executor must also handle `write_file` calls that target files that **do not yet exist**. An agent creating a new file at `src/new_module.rs` cannot have that path canonicalized because there is nothing on disk to resolve. `canonicalize()` on a non-existent path returns `Err(NotFound)`.

This is not a hypothetical edge case — it is the dominant use case. Agents write new files constantly.

---

## Decision

Use **lexical normalization** for path validation, not `fs::canonicalize()`.

The `normalize_path()` function resolves `.` and `..` components in memory by walking `Path::components()` and applying the same rules a filesystem would, without touching the disk. The result is compared against the lexically-known working directory prefix.

For the **symlink escape** vector specifically — where a symlink inside the workspace points to a location outside it — `canonicalize()` is used *only* on the nearest existing ancestor of the target path, not on the full target path. This avoids the `NotFound` failure while still catching escape via symlink.

The full validation sequence in `resolve_path()`:

1. Reject absolute paths and backslash paths immediately.
2. Walk components; reject any `ParentDir` (`..`) component.
3. Join with working directory and lexically normalize via `normalize_path()`.
4. Call `ensure_path_is_within_workspace()`, which canonicalizes only the nearest existing ancestor and verifies it is within the canonical working directory.

---

## Rationale

`canonicalize()` is the correct tool for verifying symlink safety. It is the wrong tool for validating the *intended* path when the file does not exist yet. Combining lexical normalization (for new files) with selective canonicalization (for symlink checking) gives full coverage with no false `NotFound` errors.

Rejecting `..` components at the lexical stage before any filesystem access eliminates traversal without relying on the filesystem's own path resolution, which can differ by platform.

---

## Alternatives considered

### `canonicalize()` on the full path — always

Fails for new files. Would require pre-creating empty files to validate the path, which is a worse side effect than the problem being solved.

### Allow `..` but resolve and check the final path

If an agent provides `src/../../etc/passwd`, the lexical resolver strips `src/` then `..` and leaves `etc/passwd` relative to the workspace root. Then `..` strips the workspace root entirely. This approach is fragile and platform-dependent. Rejecting `..` at the component level is simpler and eliminates an entire class of bypass.

### `std::path::absolute()` (stabilised Rust 1.79)

`Path::absolute()` resolves `.` and `..` lexically without touching the filesystem and without requiring the path to exist. It does not resolve symlinks. This is a viable alternative to our hand-rolled `normalize_path()` and should be evaluated as a replacement when the MSRV allows. The current implementation predates its stabilisation.

---

## Consequences

**Easier:**
- Agents can freely create new files anywhere within the workspace without triggering false security errors.
- Path validation is deterministic and platform-independent for the `..` traversal case.

**Harder:**
- The two-phase approach (lexical + selective canonicalize) is more complex than a single `canonicalize()` call and requires careful maintenance.
- Symlink safety is only enforced for paths whose *nearest existing ancestor* is within the workspace. A carefully constructed chain of symlinks could theoretically defeat this if the working directory itself contains a symlink — an accepted limitation for the current security model.

**Constraints imposed on future work:**
- Do not replace `normalize_path()` with `canonicalize()`. If `std::path::absolute()` becomes available, evaluate it as a drop-in but do not remove the ancestor-canonicalization symlink check.
- Any new tool that performs filesystem operations must route paths through `resolve_path()`. Bypassing it is a security regression.
- The anchor test `test_path_traversal_prevention` in `src/tools/executor.rs` must remain passing. It is the regression gate for this decision.
