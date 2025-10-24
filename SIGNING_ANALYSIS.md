# WASM Component Signing Analysis

## 🔍 Investigation Summary

This document summarizes the deep investigation into WASM component signing for the bazel-file-ops-component project.

## Root Cause Identified

### The Problem

**wasmsign2 signing was failing with "Not available in download strategy"** even though MODULE.bazel specified `strategy = "bazel"`.

### The Investigation

Through systematic code analysis, we discovered a **module extension registration conflict**:

1. **rules_wasm_component/MODULE.bazel** registers:
   ```python
   wasm_toolchain.register(
       name = "wasm_tools",
       strategy = "download",  # Default in dependency
   )
   ```

2. **bazel-file-ops-component/MODULE.bazel** registers:
   ```python
   wasm_toolchain.register(
       name = "wasm_tools",  # Same name!
       strategy = "bazel",   # Overridden by dependency
   )
   ```

3. **The Conflict**: When both modules register the same extension with the same name, the dependency's registration takes precedence in MODULE.bazel.lock

### Attempted Solution

Changed registration name to `"wasm_tools_signing"` to avoid conflict:
- ✅ Successfully switched to `strategy = "bazel"`
- ❌ Discovered **strategy = "bazel" is incomplete** in rules_wasm_component

### Secondary Problem Discovered

The "bazel" strategy implementation in rules_wasm_component is **not production-ready**:

```python
# From toolchains/wasm_toolchain.bzl:580
repository_ctx.template(
    "BUILD.wasm_tools",
    Label("//toolchains:BUILD.wasm_tools_bazel"),  # ❌ File doesn't exist!
)
```

**Missing files:**
- `toolchains/BUILD.wasm_tools_bazel`
- `toolchains/BUILD.wizer_bazel`

The Bazel-native rust_binary approach was started but never completed.

## 📊 Code Analysis

### Extension Flow

```
MODULE.bazel
  ↓
wasm_toolchain.register(name, strategy)
  ↓
_wasm_toolchain_extension_impl (extensions.bzl:13)
  ↓
Collects registrations from ALL modules
  ↓
wasm_toolchain_repository(strategy=...)
  ↓
_wasm_toolchain_repository_impl (wasm_toolchain.bzl:103)
  ↓
if strategy == "bazel":
    _setup_bazel_native_tools()  # ❌ Incomplete implementation
```

### The Lock File Issue

Even after deleting MODULE.bazel.lock, it regenerated with `"strategy": "download"` because:
1. Our registration name conflicted with rules_wasm_component's
2. The dependency's registration was processed and stored
3. Our `strategy = "bazel"` parameter was ignored

## ✅ Production Solution: OCI Signing

Since wasmsign2 signing is not production-ready, we implemented **OCI-layer signing** instead:

### Release Workflow Features

1. **Build**: Unsigned WASM component (verified functional at 1.6MB)
2. **Package**: OCI artifact using `crane`
3. **Sign**: Cosign with keyless GitHub OIDC
4. **Attest**: SLSA provenance for supply chain security
5. **Publish**: GitHub Releases + ghcr.io

### Security Model

| Layer | Technology | Status |
|-------|-----------|---------|
| WASM Component | wasmsign2 | ❌ Not ready (incomplete toolchain) |
| OCI Image | Cosign + OIDC | ✅ Production ready |
| Provenance | SLSA | ✅ Production ready |
| Checksums | SHA256 | ✅ Production ready |

### Benefits

- **No Secret Management**: Keyless signing via GitHub OIDC
- **Verifiable**: `cosign verify` with certificate transparency
- **Supply Chain Security**: SLSA provenance attestation
- **Standard Tooling**: Compatible with existing OCI registries
- **Works Today**: No dependency on incomplete toolchain features

## 🔮 Future: Dual-Layer Signing

Once rules_wasm_component completes the Bazel-native rust_binary implementation:

1. **Create missing BUILD templates**:
   - `toolchains/BUILD.wasm_tools_bazel`
   - `toolchains/BUILD.wizer_bazel`

2. **Update registration** in bazel-file-ops-component:
   ```python
   wasm_toolchain.register(
       name = "wasm_tools_signing",  # Avoid conflict
       strategy = "bazel",
   )
   ```

3. **Build signed component**:
   ```bash
   bazel build //tinygo:file_ops_component_signed
   ```

4. **Dual-layer workflow**:
   - Sign WASM with wasmsign2
   - Package signed WASM in OCI
   - Sign OCI with Cosign
   - Double verification layer

## 📝 Key Learnings

1. **Module Extension Conflicts**: Same-name registrations from dependencies override root module
2. **Lock File Persistence**: MODULE.bazel.lock caches extension results, not always updated
3. **Strategy Implementation**: Always verify the target strategy is actually implemented
4. **Pragmatic Solutions**: OCI signing provides strong security without waiting for incomplete features
5. **Deep Investigation Pays Off**: Understanding the full code path revealed the actual issue

## 🎯 Recommendations

### For bazel-file-ops-component
- ✅ Use OCI signing for Phase 1 releases
- ⏰ Add wasmsign2 in Phase 2 when toolchain is ready
- ✅ Document security model clearly

### For rules_wasm_component
- Complete the Bazel-native rust_binary implementation
- Create missing BUILD template files
- Consider changing default strategy to "hybrid" or documenting "bazel" limitations
- Add validation for incomplete strategy implementations

## 🔗 References

- [Cosign Keyless Signing](https://docs.sigstore.dev/cosign/keyless/)
- [SLSA Provenance](https://slsa.dev/provenance/)
- [OCI Artifacts](https://github.com/opencontainers/artifacts)
- [rules_wasm_component Issue Tracker](https://github.com/pulseengine/rules_wasm_component/issues)

---

**Investigation Date**: October 24, 2025
**Status**: RESOLVED - Production OCI signing implemented
**Next**: Complete rules_wasm_component Bazel strategy for dual-layer signing
