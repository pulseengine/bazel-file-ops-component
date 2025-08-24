# Security Model

## Component Signing

This project implements dual-layer cryptographic signing for WebAssembly components:

### Layer 1: Component Signing (wasmsign2)

- **Algorithm**: EdDSA with Ed25519 keys
- **Signature Type**: Embedded in WebAssembly component
- **Verification**: Built-in component signature verification
- **Key Format**: Compact format optimized for WebAssembly

### Layer 2: OCI Signing (Cosign)

- **Algorithm**: Sigstore keyless signing with GitHub OIDC
- **Signature Type**: Detached OCI manifest signatures
- **Verification**: Cosign transparency log verification
- **Identity**: Tied to GitHub repository and workflow

## Verification Process

### Verify OCI Container Signature

```bash
# Install cosign
go install github.com/sigstore/cosign/v2/cmd/cosign@latest

# Verify container signature
cosign verify --certificate-identity-regexp='.*' \
  --certificate-oidc-issuer-regexp='.*' \
  ghcr.io/pulseengine/bazel-file-ops-component/tinygo-signed:latest
```

### Verify WebAssembly Component Signature

```bash
# Install wasmsign2 (requires rules_wasm_component)
bazel run @rules_wasm_component//toolchains:wasmsign2 -- --version

# Extract component and verification key
docker run --rm -v $(pwd):/output \
  ghcr.io/pulseengine/bazel-file-ops-component/tinygo-signed:latest \
  cp /component.wasm /verification-key.pub /output/

# Verify component signature
bazel run @rules_wasm_component//toolchains:wasmsign2 -- \
  verify component.wasm --key verification-key.pub
```

## Security Properties

### Supply Chain Security

- **Identity Verification**: GitHub OIDC provides cryptographic proof of origin
- **Transparency**: All signatures recorded in public transparency logs
- **Non-Repudiation**: Signatures cannot be forged or denied
- **Integrity**: Any tampering invalidates signatures

### Runtime Security

- **Sandboxing**: WebAssembly provides memory isolation
- **Capability-Based Security**: WASI preview 2 capability model
- **Path Restrictions**: Wasmtime preopen directories limit filesystem access
- **Component Model**: Strict interface boundaries

## Trust Model

### Trusted Components

1. **GitHub Repository**: `github.com/pulseengine/bazel-file-ops-component`
2. **GitHub Workflow**: Automated signing in CI/CD pipeline
3. **Sigstore Infrastructure**: Public key infrastructure and transparency logs
4. **rules_wasm_component**: Signing toolchain and verification tools

### Trust Verification

1. Verify GitHub repository authenticity
2. Check workflow identity in transparency logs
3. Validate component signatures before use
4. Use pinned component digests in production

## Security Recommendations

### For Users

- **Always verify signatures** before using components in production
- **Pin specific component digests** rather than using latest tags
- **Use signed variants** (`tinygo-signed`) for production workloads
- **Monitor transparency logs** for unexpected signing events

### For Contributors

- **Never commit private keys** to the repository
- **Use signed commits** for security-critical changes
- **Report security issues** privately to maintainers
- **Follow secure coding practices** in component implementations

## Reporting Security Issues

Please report security vulnerabilities privately:

- **Email**: <security@pulseengine.dev>
- **GitHub**: Use private vulnerability reporting
- **Response Time**: 48 hours acknowledgment, 30 days resolution

## Security Updates

Security updates will be:

- Released as patch versions
- Documented in CHANGELOG.md
- Announced through GitHub Security Advisories
- Signed with updated signatures
