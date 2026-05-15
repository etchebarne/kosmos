# highlight

Role: shared highlight vocabulary.

Owns:
- The `HighlightId` enum used by syntax producers and rendering/theme consumers.
- Stable token categories that can be mapped to visual styles.

Does Not Own:
- Tree-sitter parsing.
- LSP semantic token transport.
- Colors, fonts, or rendering.

Keep this crate small so syntax, theme, and UI can share token categories without depending on each other.
