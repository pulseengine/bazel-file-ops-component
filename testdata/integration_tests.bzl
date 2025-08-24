"""Integration tests for file operations components"""

load("@bazel_skylib//rules:build_test.bzl", "build_test")
load("@rules_go//go:def.bzl", "go_test")

def file_ops_integration_tests(name = "integration_tests"):
    """Define integration tests for file operations components

    Args:
      name: Base name for the test suite (default: "integration_tests")
    """

    # Test that TinyGo implementation builds successfully
    build_test(
        name = "tinygo_component_build_test",
        targets = [
            "//tinygo:file_ops_tinygo",
            "//tinygo:file_ops_component_wasm",
        ],
        tags = ["integration", "build"],
    )

    # Test Rust component build (if it exists)
    native.config_setting(
        name = "rust_component_exists",
        values = {"define": "rust_enabled=true"},
    )

    build_test(
        name = "rust_component_build_test",
        targets = select({
            ":rust_component_exists": [
                "//rust:file_ops_rust",
                "//rust:file_ops_component_wasm",
            ],
            "//conditions:default": ["//tinygo:file_ops_tinygo"],  # Fallback
        }),
        tags = ["integration", "build"],
    )

    # Go-based integration tests
    go_test(
        name = "integration_tests",
        srcs = ["integration_test.go"],
        data = [
            "//tinygo:file_ops_tinygo",
            "//tinygo:file_ops_component_wasm",
            "//wit:file-operations.wit",
        ],
        env = {
            "COMPONENT_BINARY": "$(location //tinygo:file_ops_tinygo)",
            "COMPONENT_WASM": "$(location //tinygo:file_ops_component_wasm)",
            "WIT_SOURCE": "$(location //wit:file-operations.wit)",
        },
        tags = ["integration", "manual"],  # Manual due to external tool dependencies
    )

    # Lightweight component functionality test
    go_test(
        name = "component_functionality_test",
        srcs = ["integration_test.go"],
        args = ["-test.run=TestComponentBuild"],
        data = [
            "//tinygo:file_ops_tinygo",
        ],
        env = {
            "COMPONENT_BINARY": "$(location //tinygo:file_ops_tinygo)",
        },
        tags = ["integration"],
    )

    # JSON batch compatibility test
    go_test(
        name = "json_batch_compatibility_test",
        srcs = ["integration_test.go"],
        args = ["-test.run=TestJSONBatchCompatibility"],
        data = [
            "//tinygo:file_ops_tinygo",
        ],
        env = {
            "COMPONENT_BINARY": "$(location //tinygo:file_ops_tinygo)",
        },
        tags = ["integration"],
    )

    # WIT interface consistency test (requires external tools)
    go_test(
        name = "wit_interface_consistency_test",
        srcs = ["integration_test.go"],
        args = ["-test.run=TestWITInterfaceConsistency"],
        data = [
            "//tinygo:file_ops_component_wasm",
            "//wit:file-operations.wit",
        ],
        env = {
            "COMPONENT_WASM": "$(location //tinygo:file_ops_component_wasm)",
            "WIT_SOURCE": "$(location //wit:file-operations.wit)",
        },
        tags = ["integration", "wit", "manual"],  # Manual due to wasm-tools dependency
    )

    # Basic performance test
    go_test(
        name = "performance_basic_test",
        srcs = ["integration_test.go"],
        args = ["-test.run=TestPerformanceBasic"],
        data = [
            "//tinygo:file_ops_tinygo",
        ],
        env = {
            "COMPONENT_BINARY": "$(location //tinygo:file_ops_tinygo)",
        },
        tags = ["integration", "performance"],
        timeout = "moderate",  # Allow more time for performance tests
    )

    # Test suite combining all integration tests
    native.test_suite(
        name = "integration_test_suite",
        tests = [
            ":tinygo_component_build_test",
            ":component_functionality_test",
            ":json_batch_compatibility_test",
            ":performance_basic_test",
        ],
        tags = ["integration"],
    )

    # Extended test suite including manual tests (for CI)
    native.test_suite(
        name = "integration_test_suite_full",
        tests = [
            ":tinygo_component_build_test",
            ":rust_component_build_test",
            ":integration_tests",
            ":component_functionality_test",
            ":json_batch_compatibility_test",
            ":wit_interface_consistency_test",
            ":performance_basic_test",
        ],
        tags = ["integration", "full"],
    )
