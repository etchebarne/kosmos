# settings

Role: settings model, registry, and storage.

Owns:
- Setting values and typed value accessors.
- Built-in settings categories and controls.
- Settings database loading and saving.
- The GPUI global `Settings` state.

Does Not Own:
- Settings tab rendering.
- Applying settings to windows or features.
- Tool installation logic.
