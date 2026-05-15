# icons

Role: icon assets and icon rendering primitives.

Owns:
- Application asset registration.
- `IconName` and icon asset lookup.
- GPUI `Icon` rendering.
- File-name and language-id to icon mapping.
- SVG rasterization support used by icon rendering.

Does Not Own:
- Deciding where icons appear in the UI.
- Tab kind registration.
- General theme definitions beyond icon rendering needs.
