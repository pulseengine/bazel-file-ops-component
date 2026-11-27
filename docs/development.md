# Building and testing

The implementation is Go, built with TinyGo into a WASI component. There is no
Rust implementation in this repository.

## Component

```sh
bazel build //tinygo:file_ops_component     # bazel-bin/tinygo/file_ops_component.wasm
bazel test //tinygo:file_ops_test
```

`//:file_ops_component` is an alias for the same target.

## Go tests without Bazel

```sh
cd tinygo
go test ./...
```

## Checks that gate a pull request

`.github/required-contexts.txt` lists the 8 checks that must pass before a
pull request can merge into `main`. The workflows are in `.github/workflows/`:
`ci.yml` covers build, tests and integration, and `security.yml`
runs the dependency, vulnerability and supply-chain scans.

