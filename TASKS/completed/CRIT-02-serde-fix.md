# Task CRIT-02: Serde Serialization Repair

**Target File:** `src/types/api.rs`

**Issue:** The `ApiMessage` struct uses `#[serde(flatten)]` on the `content` field. This causes serialization to fail with error: "can only flatten structs and maps (got a string)" when the content is `Content::Text(String)`.

**Definition of Done:**
1. Remove `#[serde(flatten)]` from `src/types/api.rs` line 6.
2. Ensure the `test_crit_02_regression` anchor test passes.

**Context:**
When `#[serde(flatten)]` is applied to a field, serde tries to merge that field's contents into the parent object. However, the `Content` enum has a `Text(String)` variant that serializes to a plain string, which cannot be flattened. This causes a runtime serialization error.

**Error Message:**
```
Error("can only flatten structs and maps (got a string)", line: 0, column: 0)
```

**Expected JSON Output (after fix):**
```json
{
  "role": "user",
  "content": "Hello"
}
```

**Anchor Test:** `test_crit_02_regression` in `src/types/api.rs`
