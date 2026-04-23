import { readText } from "@tauri-apps/plugin-clipboard-manager";
import type { editor } from "monaco-editor";
import type { ContextMenuItem } from "../../components/shared/ContextMenu";

/** Build the standard cut/copy/paste/select-all items for the editor's custom context menu. */
export function buildContextMenuItems(
  instance: editor.IStandaloneCodeEditor | null,
): ContextMenuItem[] {
  const hasSelection = instance ? !instance.getSelection()?.isEmpty() : false;

  return [
    {
      label: "Cut",
      disabled: !hasSelection,
      onClick: () => {
        if (!instance) return;
        const sel = instance.getSelection();
        if (!sel || sel.isEmpty()) return;
        const text = instance.getModel()!.getValueInRange(sel);
        navigator.clipboard.writeText(text);
        instance.executeEdits("context-menu", [{ range: sel, text: "" }]);
        instance.focus();
      },
    },
    {
      label: "Copy",
      disabled: !hasSelection,
      onClick: () => {
        if (!instance) return;
        const sel = instance.getSelection();
        if (!sel || sel.isEmpty()) return;
        navigator.clipboard.writeText(instance.getModel()!.getValueInRange(sel));
        instance.focus();
      },
    },
    {
      label: "Paste",
      onClick: async () => {
        if (!instance) return;
        try {
          const text = await readText();
          if (text) {
            instance.trigger("context-menu", "type", { text });
          }
        } catch {
          /* clipboard empty or inaccessible */
        }
        instance.focus();
      },
    },
    { separator: true as const },
    {
      label: "Select All",
      onClick: () => {
        if (!instance) return;
        const model = instance.getModel();
        if (!model) return;
        const lastLine = model.getLineCount();
        const lastCol = model.getLineMaxColumn(lastLine);
        instance.setSelection({
          startLineNumber: 1,
          startColumn: 1,
          endLineNumber: lastLine,
          endColumn: lastCol,
        });
        instance.focus();
      },
    },
  ];
}
