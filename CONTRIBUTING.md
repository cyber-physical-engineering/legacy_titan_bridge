# Contributing

This is a prototype. Issues and pull requests are welcome. There is no release schedule and no promised response time.

## Build and test

```bash
# COBOL library
cd cobol && cobc -m -o libcorebanking.so core_banking.cbl && cd ..

# Rust sidecar
cd sidecar
cargo fmt --check
cargo clippy -- -D warnings
cargo test
COBOL_LIB_PATH=../cobol/libcorebanking.so cargo run
```

The contract compiles with `npx -y solc@0.8.24 --bin --abi --base-path contracts -o build/contracts contracts/Anchor.sol`, which is what CI runs.

## Before you open a pull request

1. Run the steps above. `cargo fmt` and `cargo clippy -- -D warnings` must be clean; CI fails otherwise.
2. Keep the change small and say what it fixes.
3. Keep `Cargo.lock` committed; this is a binary crate.
4. If the change removes a limit listed in the README, update that section.

## Ideas that fit

- A duplicate check on transaction IDs.
- An endpoint that returns a Merkle proof for one leaf (`merkle.rs` already computes proofs).
- Authentication on the endpoints.
