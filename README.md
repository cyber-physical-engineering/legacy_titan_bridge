# Legacy Titan Bridge

A demo in three parts. A Rust web service takes a bank transfer as JSON, scores it with a small risk policy, and runs it through a COBOL routine. It then adds the transfer's SHA-256 hash to a Merkle tree. A Solidity contract stores Merkle roots on Ethereum. The COBOL module simulates account checks.

**Status: prototype.** 12 Rust tests pass, and `cargo fmt --check` and `cargo clippy -- -D warnings` are clean. The service was run against the compiled COBOL library with GnuCOBOL 3.2, and `Anchor.sol` compiles with solc 0.8.24. All of that was re-run in October 2026. CI builds the image, starts the sidecar and an Anvil node with Docker Compose, and checks that /health reports the COBOL library loaded and the node reachable.

[![CI](https://github.com/cyber-physical-engineering/legacy_titan_bridge/actions/workflows/ci.yml/badge.svg)](https://github.com/cyber-physical-engineering/legacy_titan_bridge/actions/workflows/ci.yml)

James Thornton set the architecture and requirements. The code was written with AI-assisted development in late 2025. The tests and checks were re-run in October 2026.

## The three parts

| Part | Where | What it does |
|---|---|---|
| Sidecar | `sidecar/` (Rust, Axum) | Four HTTP endpoints, the risk policy, the COBOL call through a shared library, the Merkle tree, the Ethereum client |
| COBOL module | `cobol/core_banking.cbl` | The `PROCESS-TX` entry point with simulated account checks; status codes `00` to `04` and `99` |
| Contract | `contracts/Anchor.sol` | Stores Merkle roots, append-only, with an owner, a submitter role and a pause switch |

## Quick start

Build the COBOL library and start the service:

```bash
cd cobol && cobc -m -o libcorebanking.so core_banking.cbl && cd ..
cd sidecar && COBOL_LIB_PATH=../cobol/libcorebanking.so cargo run
```

Run the tests:

```bash
cd sidecar && cargo test
```

With Docker Compose, which starts the sidecar on port 3000 and an Anvil Ethereum node on port 8545 (chain ID 31337):

```bash
docker compose up --build
```
CI runs this on every push.

Deploying the contract is a separate, opt-in step: `docker compose --profile deploy up deploy-contracts`. It was not run in October 2026. The sidecar anchors a root only when `ANCHOR_CONTRACT` holds the deployed address; with it empty, a commit returns "Contract not configured".

If the COBOL library is not found, the service starts anyway and answers every transfer with `SUCCESS: Simulated COBOL processing`. `/health` shows which mode you are in.

## Endpoints

| Method | Path | What it does |
|---|---|---|
| POST | `/transfer` | Score the transfer, call COBOL, add the hash to the tree |
| POST | `/commit-batch` | Submit the current Merkle root to the contract, then reset the tree |
| GET | `/health` | Whether the COBOL library loaded and whether an Ethereum node answers |
| GET | `/tree-status` | Leaf count, pending commits, last root and last commit time |

Responses from a local run with the COBOL library loaded and no Ethereum node (October 2026):

```bash
curl -s localhost:3000/health
```

```json
{"status":"healthy","version":"0.1.0","cobol_available":true,"blockchain_connected":false}
```

```bash
curl -s -X POST localhost:3000/transfer -H 'Content-Type: application/json' \
  -d '{"transaction_id":"TX-2026-001","amount":15000.00,"from_account":"ACC-001","to_account":"ACC-002"}'
```

```json
{"status":"pending_proof","transaction_id":"TX-2026-001","risk_level":"HIGH","risk_reason":"Amount $15000.00 exceeds high-risk threshold $10000.00; Suspiciously round amount detected","tree_position":0,"cobol_status":"00","cobol_message":"SUCCESS: Transfer of $00000001500000 completed","tx_hash":"a390e38fbb950dcaeadfd8530b8d81c0b3464a6cae5130123dd69defa71c4591"}
```

```bash
curl -s localhost:3000/tree-status
```

```json
{"leaf_count":1,"pending_commits":1,"last_root":null,"last_commit_time":null}
```

```bash
curl -s -X POST localhost:3000/commit-batch
```

```json
{"status":"error","merkle_root":"a390e38fbb950dcaeadfd8530b8d81c0b3464a6cae5130123dd69defa71c4591","leaf_count":1,"error":"Blockchain submission failed: Not connected to Ethereum node"}
```

With no node, the commit returns HTTP 500 with the root it computed, and the tree keeps its leaves so you can retry.

## The risk policy

From `sidecar/src/policy.rs`. Each rule adds to a score, and the score sets the level.

- An amount at or above $5,000 adds 15; at or above $10,000 adds 30; at or above $100,000 adds 50. `RISK_THRESHOLD` moves the $10,000 line.
- A round amount adds 10: an exact multiple of $1,000, or an exact $500 step between $9,000 and $9,999.
- Accounts whose first four characters match add 5.
- A score of 0 to 10 is LOW, 11 to 25 MEDIUM, 26 to 50 HIGH, and above 50 CRITICAL.

The velocity settings in the configuration (`enable_velocity_check`, `max_tx_per_minute`) have no logic behind them.

## The COBOL module

`core_banking.cbl` checks for a zero amount, a missing ID and the same account on both sides. Balances are pseudo-random, derived from the first character of the account ID, and about one account in ten reads as frozen by the same rule. It applies a single-transaction limit. Its own "transaction hash" field is concatenated text, not a cryptographic hash. The real SHA-256 leaf is computed in Rust (`merkle::hash_transaction`) over the ID, the amount's bytes and both account IDs.

GnuCOBOL exports `ENTRY "PROCESS-TX"` as the C symbol `PROCESS__TX`. Its runtime must be started once with `cob_init`, and it is not thread-safe. The sidecar does both and serializes calls through a mutex.

## The contract

`Anchor.sol` keeps an array of roots with a timestamp, leaf count and block number for each. Only the submitter address can add a root. The owner can change the submitter, pause the contract and transfer ownership. There is no function to change or remove a stored root. Roots are stored whenever the submitter calls; nothing schedules a daily submission.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `SIDECAR_PORT` | `3000` | HTTP port |
| `COBOL_LIB_PATH` | `/usr/lib/libcorebanking.so` | Path to the compiled COBOL library |
| `ETH_RPC_URL` | `http://localhost:8545` | Ethereum JSON-RPC endpoint |
| `ANCHOR_CONTRACT` | none | Address of the deployed `Anchor.sol` |
| `PRIVATE_KEY` | none | Key that signs root submissions |
| `RISK_THRESHOLD` | `10000` | Amount that sets the HIGH line |

## Limits

- No authentication and no rate limiting on the endpoints.
- No replay check. The same transaction ID is accepted twice.
- The signing key comes from an environment variable. A real deployment needs an HSM or a vault.
- Committing a root needs an Ethereum node and `ANCHOR_CONTRACT` set to a deployed contract address. The compose file starts a node; the deploy step is opt-in and was not run in October 2026.
- The FFI is `unsafe` and hands raw pointers to COBOL. Rust's ownership rules do not reach the COBOL side.
- Merkle proof generation exists in `merkle.rs`, but no endpoint exposes it.
- The COBOL account checks are simulated. There is no ledger behind them.
- Nothing here has been measured for throughput or cost.

## License

Apache 2.0. See [LICENSE](LICENSE).
