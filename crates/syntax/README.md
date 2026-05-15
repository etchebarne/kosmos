# syntax

Role: tree-sitter syntax highlighting.

Owns:
- Grammar registration and loading.
- Syntax snapshot storage per editor buffer.
- Mapping tree-sitter captures to `HighlightId` spans.
- Incremental syntax data consumed by editor rendering.

Does Not Own:
- Text buffer editing.
- Theme colors.
- Rendering highlighted text.
- LSP semantic features.
