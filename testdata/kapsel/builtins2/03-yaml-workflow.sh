# A GitHub workflow edited without yq or python: every comment survives.
mkdir -p .github/workflows
cat > .github/workflows/ci.yml <<'EOF'
# CI for the demo crate
name: ci
on:
  push:
    branches: [main]   # only main
jobs:
  test:
    runs-on: ubuntu-22.04  # pinned image
    steps:
      # check out first
      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6
      - run: cargo test
  lint:
    runs-on: ubuntu-22.04
    steps:
      - run: cargo clippy
EOF
yaml get .jobs.test.runs-on .github/workflows/ci.yml
yaml get '.jobs.test.steps[1].run' .github/workflows/ci.yml
yaml set .jobs.test.runs-on ubuntu-24.04 .github/workflows/ci.yml
yaml set '.jobs.test.steps[2]' '{run: cargo doc}' .github/workflows/ci.yml
yaml set .jobs.test.timeout-minutes 30 .github/workflows/ci.yml
yaml del .jobs.lint .github/workflows/ci.yml
yaml get -j .on .github/workflows/ci.yml
