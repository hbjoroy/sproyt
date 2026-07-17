# Migrations

Sproyt keeps separate migration directories for PostgreSQL and SQLite while preserving one logical domain model.

```text
migrations/postgres/
migrations/sqlite/
```

PostgreSQL is the production contract. SQLite is a lightweight development mode.

Important rules:

- Application code generates ids; migrations do not depend on database-specific UUID generators.
- Enum-like values use `text` plus `check` constraints in the first version.
- Message sequence is per channel and allocated through `channel_sequences`.
- Read state uses `last_read_sequence`, not `last_read_message_id`.
- SQLite connections must enable foreign keys with `PRAGMA foreign_keys = ON`.
- The shared repository contract runs against both dialects in CI.
- Agent rate limits are database-authoritative so all application replicas
  consume one fixed 60-second window per agent.
