# Agent 06 — Testing

## Obiectiv
100% coverage înainte ca UI client să fie funcțional. Nu înseamnă 100% linii mecanic, ci fiecare comportament semnificativ este testat.

## Piramidă
1. **Unit tests** în fiecare crate (`mod tests`).
2. **Integration tests** în `tests/` pentru desktop ↔ worker.
3. **E2E** manual/CI cu worker real pe container/VM.

## Reguli
- Fără `#[ignore]` fără explicație în fișier `.agents`.
- Fiecare test izolează starea (tempfile, mockall).
- Fiecare eroare publică are test de eroare.
- Fiecare roundtrip de date este testat.
- Fiecare funcție crypto este testată cu vectori cunoscuți și cu fuzz/proptest.

## Coverage
- Rulează cu `cargo llvm-cov`.
- Target: >= 95% line coverage, 100% module coverage.
- Ignorăm doar liniile `unreachable!` și implementări de trait passthrough.

## CI
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --check`
- `cargo build --release` pentru goblin-worker (target musl)

## Mocking
- `mockall` pentru trait-uri externe (LLM provider, secret store, SSH client).
- Servere de test axum pentru worker.
- Subprocesse mock pentru MCP.
