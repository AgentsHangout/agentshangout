#!/usr/bin/env bash
# lit starts this command inside each new candidate checkout.
set -eu
python3 -m unittest discover -s .github/tests -v
docker run --rm -v "$PWD:/work:ro" -w /work \
  rhysd/actionlint@sha256:b1934ee5f1c509618f2508e6eb47ee0d3520686341fec936f3b79331f9315667 -color

# Cargo uses 101 for a failed check. lit bisects on exit 1 and stops on
# infrastructure errors, including timeout's 124, so a stuck run keeps its log.
trap 'rc=$?; if [[ $rc == 101 ]]; then exit 1; fi; exit "$rc"' ERR
cargo fmt --check
time cargo build --locked --all-targets
time timeout --kill-after=30s 20m cargo test --locked
time cargo clippy --locked --all-targets -- -D warnings
