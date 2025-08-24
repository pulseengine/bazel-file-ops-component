# Integration Guide: Updating rules_wasm_component

This guide shows how to update `rules_wasm_component` to use the external `bazel-file-ops-component` instead of its built-in file operations implementation.

## Overview

The `rules_wasm_component` repository currently has its own internal file operations component at:

- `tools/file_operations_component/` - Internal implementation
- `tools/bazel_helpers/file_ops_actions.bzl` - Bazel action helpers
- `toolchains/file_ops_toolchain.bzl` - Toolchain configuration

We need to update these to use the external `bazel-file-ops-component` for better security, performance, and maintainability.

## Required Changes

### 1. Update MODULE.bazel

Add the external file operations component as a dependency:

```python
# In rules_wasm_component/MODULE.bazel

# Add bazel-file-ops-component dependency
bazel_dep(name = "bazel_file_ops_component", version = "1.0.0")

# Git override for development (use specific tag/commit for production)
git_override(
    module_name = "bazel_file_ops_component",
    remote = "https://github.com/pulseengine/bazel-file-ops-component.git",
    commit = "v1.0.0",  # Use stable release
)
```

### 2. Update File Operations Toolchain

Modify `toolchains/file_ops_toolchain.bzl` to use the external component:

```python
# toolchains/file_ops_toolchain.bzl

def _file_ops_toolchain_repository_impl(repository_ctx):
    """Implementation of file_ops_toolchain_repository rule"""

    # Create BUILD file for the toolchain
    repository_ctx.file("BUILD.bazel", """
load("@rules_wasm_component//toolchains:file_ops_toolchain.bzl", "file_ops_toolchain")

# File Operations Toolchain using external bazel-file-ops-component
file_ops_toolchain(
    name = "file_ops_toolchain_impl",
    # CHANGED: Use external component instead of internal one
    file_ops_component = "@bazel_file_ops_component//tools:file_ops",
    wit_files = ["@bazel_file_ops_component//wit:file-operations.wit"],
    visibility = ["//visibility:public"],
)

# Rest of the toolchain configuration remains the same...
toolchain(
    name = "file_ops_toolchain",
    exec_compatible_with = [
        "@platforms//os:linux",
        "@platforms//os:macos",
        "@platforms//os:windows",
    ],
    target_compatible_with = [
        "@platforms//cpu:wasm32",
    ],
    toolchain = ":file_ops_toolchain_impl",
    toolchain_type = "@rules_wasm_component//toolchains:file_ops_toolchain_type",
    visibility = ["//visibility:public"],
)
""")
```

### 3. Update Bazel Actions Helper

Update `tools/bazel_helpers/file_ops_actions.bzl` to use the improved external API:

```python
# tools/bazel_helpers/file_ops_actions.bzl

def file_ops_action(ctx, operation, **kwargs):
    """Execute a file operation using the external File Operations Component

    This now uses the enhanced bazel-file-ops-component with better security,
    performance, and cross-platform compatibility.
    """

    # Get the file operations component from toolchain (now external)
    file_ops_toolchain = ctx.toolchains["@rules_wasm_component//toolchains:file_ops_toolchain_type"]
    file_ops_component = file_ops_toolchain.file_ops_component

    if not file_ops_component:
        fail("File operations component not available in toolchain")

    # Use the enhanced external component API
    if operation == "copy_file":
        return _copy_file_action(ctx, file_ops_component, **kwargs)
    elif operation == "copy_directory":
        return _copy_directory_action(ctx, file_ops_component, **kwargs)
    elif operation == "create_directory":
        return _create_directory_action(ctx, file_ops_component, **kwargs)
    elif operation == "prepare_workspace":
        return _prepare_workspace_action(ctx, file_ops_component, **kwargs)
    else:
        fail("Unsupported file operation: {}".format(operation))

def _copy_file_action(ctx, component, **kwargs):
    """Enhanced copy_file using external component"""
    src = kwargs.get("src")
    dest = kwargs.get("dest")

    if not src or not dest:
        fail("copy_file requires 'src' and 'dest' arguments")

    # Handle File objects vs strings
    inputs = []
    outputs = []

    if hasattr(src, "path"):
        inputs.append(src)
        src_path = src.path
    else:
        src_path = src

    if hasattr(dest, "path"):
        outputs.append(dest)
        dest_path = dest.path
    else:
        dest_file = ctx.actions.declare_file(dest)
        outputs.append(dest_file)
        dest_path = dest_file.path

    # Use enhanced external component with security features
    ctx.actions.run(
        executable = component,
        arguments = [
            "copy_file",
            "--src", src_path,
            "--dest", dest_path,
            "--security-level", "high",  # Enhanced security
            "--implementation", "auto",   # Smart implementation selection
        ],
        inputs = inputs,
        outputs = outputs,
        mnemonic = "FileOpsCopyFile",
        progress_message = "Copying file {} to {} for {}".format(src_path, dest_path, ctx.label),
    )

    return outputs[0] if outputs else None

def _prepare_workspace_action(ctx, component, config):
    """Enhanced workspace preparation using external component with JSON batch processing"""

    # Create workspace output directory
    workspace_dir = ctx.actions.declare_directory(config["work_dir"])

    # Create enhanced configuration for external component
    enhanced_config = {
        "workspace_dir": workspace_dir.path,
        "security": {
            "level": "high",
            "allowed_paths": [workspace_dir.path],
        },
        "operations": []
    }

    # Add file operations to config
    for source_info in config.get("sources", []):
        enhanced_config["operations"].append({
            "type": "copy_file",
            "src_path": source_info["source"].path,
            "dest_path": workspace_dir.path + "/" + (source_info.get("destination") or source_info["source"].basename),
            "preserve_metadata": source_info.get("preserve_permissions", True),
        })

    for header_info in config.get("headers", []):
        enhanced_config["operations"].append({
            "type": "copy_file",
            "src_path": header_info["source"].path,
            "dest_path": workspace_dir.path + "/" + (header_info.get("destination") or header_info["source"].basename),
            "preserve_metadata": header_info.get("preserve_permissions", True),
        })

    # Create config file
    config_file = ctx.actions.declare_file(ctx.label.name + "_workspace_config.json")
    ctx.actions.write(
        output = config_file,
        content = json.encode(enhanced_config),  # Use proper JSON encoding
    )

    # Collect all inputs
    all_inputs = [config_file]
    for source_info in config.get("sources", []):
        all_inputs.append(source_info["source"])
    for header_info in config.get("headers", []):
        all_inputs.append(header_info["source"])

    # Use external component with JSON batch processing (much more efficient!)
    ctx.actions.run(
        executable = component,
        arguments = [
            "process_json_config",
            "--config", config_file.path,
            "--implementation", "rust",  # Use Rust for better JSON performance
            "--security-level", "high",
        ],
        inputs = all_inputs,
        outputs = [workspace_dir],
        mnemonic = "PrepareWorkspace",
        progress_message = "Preparing {} workspace for {} using enhanced component".format(
            config.get("workspace_type", "generic"),
            ctx.label,
        ),
    )

    return workspace_dir
```

### 4. Remove Internal Implementation

Once the external component is integrated and tested, remove the internal implementation:

```bash
# In rules_wasm_component repository:

# Remove internal file operations component
rm -rf tools/file_operations_component/

# Update any BUILD files that reference the old component
# Update documentation to reference the external component
```

### 5. Update Documentation

Update the README and documentation to mention the external dependency:

```markdown
# rules_wasm_component

## Dependencies

This rule set uses the following external components:

- **bazel-file-ops-component**: Secure, cross-platform file operations via WebAssembly
  - Repository: https://github.com/pulseengine/bazel-file-ops-component
  - Provides enhanced security, performance, and cross-platform compatibility
  - Replaces shell scripts with sandboxed WebAssembly components

## File Operations

File operations in rules_wasm_component are powered by the external
bazel-file-ops-component, which provides:

- **Enhanced Security**: WebAssembly sandboxing with capability-based security
- **Cross-Platform**: Works identically on Linux, macOS, and Windows
- **Performance**: Dual TinyGo/Rust implementations for optimal performance
- **JSON Batch Processing**: Efficient batch operations for complex workflows
```

## Migration Benefits

### Enhanced Security

- **WebAssembly Sandboxing**: All file operations run in isolated WASM runtime
- **Capability-Based Security**: Only explicitly allowed file system access
- **Path Validation**: Automatic protection against path traversal attacks

### Better Performance

- **Smart Implementation Selection**: Automatic choice between TinyGo (security) and Rust (performance)
- **JSON Batch Processing**: Single component call for complex operations instead of multiple shell commands
- **Efficient I/O**: Optimized file handling with streaming support

### Improved Cross-Platform Compatibility

- **No Shell Dependencies**: Eliminates platform-specific shell script issues
- **Universal Binary**: Same component works on Linux, macOS, and Windows
- **Consistent Behavior**: Identical file operations across all platforms

### Better Maintainability

- **External Maintenance**: Dedicated repository with focused development
- **Comprehensive Testing**: Extensive test suite for file operations
- **Documentation**: Complete API documentation and examples

## Testing the Integration

### 1. Basic Functionality Test

```bash
# In rules_wasm_component repository after integration:

# Test basic file operations
bazel test //test/integration:file_ops_test

# Test workspace preparation
bazel test //test/integration:workspace_test

# Test cross-platform compatibility
bazel test //test/integration:cross_platform_test
```

### 2. Performance Comparison

```bash
# Compare old vs new implementation
bazel test //test/performance:file_ops_benchmark

# Test large file operations
bazel test //test/performance:large_file_test

# Test batch operations
bazel test //test/performance:batch_ops_test
```

### 3. Security Validation

```bash
# Test security restrictions
bazel test //test/security:path_traversal_test

# Test sandbox isolation
bazel test //test/security:sandbox_test

# Test capability restrictions
bazel test //test/security:capability_test
```

## Implementation Timeline

### Phase 1: Preparation (1-2 days)

- [ ] Add bazel-file-ops-component dependency to MODULE.bazel
- [ ] Update toolchain configuration to use external component
- [ ] Test basic integration without removing internal component

### Phase 2: Enhanced Integration (2-3 days)

- [ ] Update file_ops_actions.bzl to use enhanced external API
- [ ] Implement JSON batch processing for workspace preparation
- [ ] Add security configurations and smart implementation selection

### Phase 3: Testing and Validation (2-3 days)

- [ ] Run comprehensive test suite
- [ ] Performance benchmarking
- [ ] Security validation
- [ ] Cross-platform testing

### Phase 4: Cleanup and Documentation (1-2 days)

- [ ] Remove internal file operations component
- [ ] Update documentation and examples
- [ ] Create migration guide for users

## Rollback Plan

If issues are discovered during integration, the rollback process is:

1. **Revert MODULE.bazel changes** to remove external dependency
2. **Restore toolchain configuration** to use internal component
3. **Revert file_ops_actions.bzl** to original implementation
4. **Keep internal component** until issues are resolved

The modular design ensures a clean rollback path without breaking existing functionality.

## Summary

This integration will:

1. **Enhance Security**: Move from shell scripts to sandboxed WebAssembly components
2. **Improve Performance**: Smart implementation selection and batch processing
3. **Better Cross-Platform**: Eliminate platform-specific shell script issues
4. **Reduce Maintenance**: Use dedicated external component with focused development
5. **Provide Better API**: Enhanced operations with security and performance options

The changes are designed to be backward-compatible while providing significant improvements in security, performance, and cross-platform reliability.
