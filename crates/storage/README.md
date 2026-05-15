# storage

Role: shared SQLite database plumbing.

Owns:
- Kosmos data-directory database path resolution.
- SQLite connection initialization.
- Common pragmas and migration execution.
- Thread-safe global connection slots used by higher-level storage crates.

Does Not Own:
- Table schemas.
- Domain-specific load/save functions.
- Settings or session data models.
