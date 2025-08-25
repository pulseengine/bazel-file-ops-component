# Bazel File Operations Component

Universal file operations for Bazel build systems via WebAssembly components with enhanced security and cross-platform support.

<!-- Multi-file Go component compilation now supported -->

[![CI Status](https://github.com/pulseengine/bazel-file-ops-component/workflows/CI/badge.svg)](https://github.com/pulseengine/bazel-file-ops-component/actions)
[![Documentation](https://img.shields.io/badge/docs-available-blue.svg)](https://bazel-file-ops.pulseengine.eu)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

## Overview

This repository provides WebAssembly components for secure, cross-platform file operations in Bazel build systems. It replaces shell scripts and platform-specific file operations with sandboxed WebAssembly components that work consistently across Linux, macOS, and Windows.

## Key Features

- **🔒 Enhanced Security**: WebAssembly sandboxing with wasmtime preopen directories
- **🌍 Cross-Platform**: Works identically on Linux, macOS, and Windows
- **⚡ Dual Implementation**: Choose between TinyGo (security-focused) and Rust (performance-optimized)
- **🔄 Backward Compatible**: Supports existing JSON batch processing workflows
- **🎯 Individual Operations**: Direct function calls via WIT interface
- **🏗️ Build System Integration**: Easy integration with any Bazel rule set

## Quick Start

### Installation

Add to your `MODULE.bazel`:

```starlark
bazel_dep(name = "bazel-file-ops-component", version = "0.1.0")
```

### Basic Usage

```starlark
load("@bazel_file_ops_component//toolchain:defs.bzl", "file_ops_action")

# Simple file copying with TinyGo component (high security)
file_ops_action(
    name = "copy_sources",
    implementation = "tinygo",
    operation = "copy_file",
    src = "source.cpp",
    dest = "workspace/source.cpp",
)

# Batch operations with JSON config (backward compatibility)
file_ops_action(
    name = "setup_workspace",
    implementation = "auto",
    config = "workspace_config.json",
    security_level = "high",
)
```

### JSON Configuration (Backward Compatible)

```json
{
  "workspace_dir": "/build/workspace",
  "operations": [
    {"type": "copy_file", "src_path": "/src/main.cpp", "dest_path": "main.cpp"},
    {"type": "mkdir", "path": "include/foundation"},
    {"type": "copy_directory_contents", "src_path": "/headers", "dest_path": "include"}
  ]
}
```

## Architecture

### Dual Implementation Strategy

| Implementation | Best For | Security | Performance | Use Cases |
|---------------|----------|----------|-------------|-----------|
| **TinyGo** | Security-critical operations | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | Cross-package headers, sensitive file ops |
| **Rust** | Performance-critical operations | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | Large file operations, bulk processing |

### Security Model

- **WASM Sandboxing**: Components run in isolated WebAssembly environment
- **Preopen Directories**: Only specified directories are accessible
- **Capability-Based Security**: No access outside designated paths
- **Path Validation**: Runtime validation against traversal attacks

## Implementation Selection

### Automatic Selection

```starlark
file_ops_action(
    implementation = "auto",  # Chooses best implementation based on operation
    security_level = "high",   # Influences selection criteria
)
```

### Manual Selection

```starlark
# High security operations
file_ops_action(implementation = "tinygo", security_level = "strict")

# Performance critical operations
file_ops_action(implementation = "rust", security_level = "standard")
```

## Integration Examples

### With rules_rust

```starlark
rust_wasm_component_library(
    name = "my_component",
    srcs = ["src/lib.rs"],
    deps = ["@crates//:dep"],
    workspace_preparation = "@bazel_file_ops_component//:file_ops_component",
)
```

### With rules_go

```starlark
go_wasm_component_library(
    name = "my_component",
    srcs = ["main.go"],
    workspace_preparation = "@bazel_file_ops_component//:file_ops_component",
)
```

### With rules_cc

```starlark
cc_wasm_component_library(
    name = "my_component",
    srcs = ["main.cpp"],
    hdrs = ["include/header.h"],
    workspace_preparation = "@bazel_file_ops_component//:file_ops_component",
)
```

## Documentation

- **[📚 Full Documentation](https://bazel-file-ops.pulseengine.eu)** - Complete guide with examples
- **[🏗️ Architecture Overview](https://bazel-file-ops.pulseengine.eu/architecture/overview)** - Technical architecture details
- **[🔒 Security Model](https://bazel-file-ops.pulseengine.eu/security/wasm-sandbox)** - Security features and configuration
- **[🚀 Integration Guide](https://bazel-file-ops.pulseengine.eu/integration/rules-wasm-component)** - Step-by-step integration
- **[📖 API Reference](https://bazel-file-ops.pulseengine.eu/reference/wit-interface)** - Complete WIT interface documentation

## Supported Operations

### Individual Operations

- `copy-file`: Copy single files with permissions
- `copy-directory`: Recursive directory copying
- `create-directory`: Safe directory creation
- `path-exists`: Path existence and type checking
- `validate-path`: Security validation
- `list-directory`: Directory listing with patterns

### Batch Operations

- `prepare-workspace`: Complete workspace setup
- `process-json-config`: JSON batch processing (backward compatibility)
- `setup-cpp-workspace`: C/C++ specific workspace preparation
- `setup-go-module`: Go/TinyGo module organization

## Security Configuration

### Security Levels

```starlark
# Standard: Basic path validation
file_ops_action(security_level = "standard")

# High: Strict validation + preopen directories
file_ops_action(security_level = "high")

# Strict: Maximum restrictions + minimal access
file_ops_action(security_level = "strict")
```

### Preopen Directory Configuration

```starlark
file_ops_action(
    security_config = {
        "allowed_dirs": ["/workspace", "/tmp/build"],
        "denied_patterns": ["../*", "/*.secret"],
        "enforce_validation": True,
    }
)
```

## Performance

| Operation | TinyGo Component | Rust Component | Native Binary |
|-----------|------------------|----------------|---------------|
| Single file copy | ~2ms overhead | ~1ms overhead | Baseline |
| Directory copy (100 files) | ~15ms overhead | ~8ms overhead | Baseline |
| Workspace setup | ~25ms overhead | ~12ms overhead | Baseline |

*Overhead measurements include WASM runtime initialization and security validation*

## Development

### Building Components

```bash
# Build TinyGo component
bazel build //tinygo:file_ops_component --config=tinygo

# Build Rust component
bazel build //rust:file_ops_component --config=rust-wasm

# Build both components
bazel build //... --config=wasm
```

### Running Tests

```bash
# Run all tests
bazel test //...

# Test specific implementation
bazel test //tinygo:all
bazel test //rust:all

# Integration tests
bazel test //tests/integration:all
```

### Documentation Development

```bash
# Start development server
bazel run //docs-site:dev

# Build documentation
bazel build //docs-site:build

# Deploy documentation
bazel run //docs-site:deploy
```

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Setup

```bash
# Clone repository
git clone https://github.com/pulseengine/bazel-file-ops-component.git
cd bazel-file-ops-component

# Run setup script
./scripts/setup-dev.sh

# Verify setup
bazel build //... --config=dev
bazel test //... --config=dev
```

## Ecosystem Integration

This component is designed for use across the Bazel ecosystem:

- **[rules_wasm_component](https://github.com/pulseengine/rules_wasm_component)** - Primary integration
- **rules_rust** - Rust component builds
- **rules_go** - Go component builds
- **rules_cc** - C++ component builds
- **rules_js** - JavaScript component builds

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.

## Support

- **GitHub Issues**: [Report bugs and request features](https://github.com/pulseengine/bazel-file-ops-component/issues)
- **Discussions**: [Community discussions](https://github.com/pulseengine/bazel-file-ops-component/discussions)
- **Documentation**: [Full documentation site](https://bazel-file-ops.pulseengine.eu)

---

Built with ❤️ by the [Pulse Engine](https://pulseengine.eu) team for the Bazel community.
