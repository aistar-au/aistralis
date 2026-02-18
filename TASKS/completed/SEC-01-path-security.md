# Task SEC-01: Path Traversal Protection
**Target File:** src/tools/executor.rs

**Problem:** The current tool executor blindly joins paths. An AI could potentially provide a path like "../../.ssh/id_rsa" to read sensitive system files.

**Requirements:**
1. Implement a strict path validation check in `resolve_path`.
2. Reject any paths containing ".." components.
3. Ensure the resolved path is strictly a sub-path of the configured workspace.
4. Use lexical normalization instead of canonicalization to allow for new file creation.

**Definition of Done:**
- Anchor test 'test_path_traversal_prevention' passes.
- Attempting to write to a file outside the workspace returns an Error.
