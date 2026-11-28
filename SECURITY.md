# Security Model

## Release signing

Each release publishes `file_ops_component.wasm` and
`file_ops_component_aot.wasm` as GitHub release assets with sha256 files, and
as OCI artifacts at `ghcr.io/pulseengine/kapsel:<tag>` and `:<tag>-aot`. The
pre-rename path `ghcr.io/pulseengine/bazel-file-ops-component` receives the
same artifacts through v0.3.x.

- **OCI signing (Cosign)**: Sigstore keyless signing with GitHub OIDC, plus a
  SLSA provenance attestation, for every published reference on both paths.
  The release job verifies both before it uploads the assets and fails if
  either is missing (`.github/workflows/release.yml`).
- **Component signing (wasmsign2)**: not done. The component itself carries no
  embedded signature.

## Verification

### Release asset

```bash
gh release download <tag> --repo pulseengine/kapsel -p 'file_ops_component*'
shasum -a 256 -c file_ops_component.wasm.sha256
```

### OCI signature and provenance

```bash
IDENTITY='^https://github\.com/pulseengine/kapsel/\.github/workflows/release\.yml@'
cosign verify \
  --certificate-identity-regexp="$IDENTITY" \
  --certificate-oidc-issuer=https://token.actions.githubusercontent.com \
  ghcr.io/pulseengine/kapsel:<tag>
cosign verify-attestation --type slsaprovenance \
  --certificate-identity-regexp="$IDENTITY" \
  --certificate-oidc-issuer=https://token.actions.githubusercontent.com \
  ghcr.io/pulseengine/kapsel:<tag>
```

## Security Properties

### Supply Chain Security

- **Identity Verification**: GitHub OIDC ties each signature to this
  repository's release workflow
- **Transparency**: All signatures are recorded in the public Sigstore log
- **Integrity**: Any change to a signed artifact invalidates its signature
- **Dependencies**: `cargo audit` and `cargo deny` gate every pull request
  (`.github/workflows/security.yml`, `deny.toml`)

### Runtime Security

- **Sandboxing**: WebAssembly provides memory isolation
- **Capability-Based Security**: the WASI preview 2 capability model
- **Path Restrictions**: the component opens only the directories the runtime
  preopens, such as `--dir` in wasmtime

## Trust Model

1. **GitHub Repository**: `github.com/pulseengine/kapsel`
2. **GitHub Workflow**: `.github/workflows/release.yml` builds, signs and
   verifies every release
3. **Sigstore Infrastructure**: keyless certificates and the transparency log

## Security Recommendations

### For Users

- **Verify signatures** before using components in production
- **Pin specific digests or tags** rather than `latest`
- **Load AOT code only with the matching wasmtime**: the `aot-*` sections of
  `file_ops_component_aot.wasm` are wasmtime-precompiled code and load only in
  the version that compiled them (50.0.3 from v0.3.0)

### For Contributors

- **Never commit private keys** to the repository
- **Use signed commits** for security-critical changes
- **Report security issues** privately to maintainers

## Reporting Security Issues

Please report security vulnerabilities privately:

- **Email**: <security@pulseengine.dev>
- **GitHub**: Use private vulnerability reporting
- **Response Time**: 48 hours acknowledgment, 30 days resolution

## Security Updates

Security updates will be:

- Released as patch versions
- Announced through GitHub Security Advisories
- Signed like every other release
