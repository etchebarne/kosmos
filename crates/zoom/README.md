# zoom

Role: UI zoom behavior.

Owns:
- Zoom setting id and supported zoom range.
- Reading and writing zoom percentage from settings.
- Applying zoom by changing the GPUI window rem size.
- Zoom-related GPUI actions and action wiring.

Does Not Own:
- Settings registry UI.
- General theme colors.
- Component layout decisions.
