const EDITOR_SELECTOR = "[data-kosmos-editor]";

export function installEditorMiddleClickPasteGuard(document: Document): () => void {
  let middleClickTarget: Node | null = null;
  let resetTimer: ReturnType<typeof setTimeout> | null = null;

  const clearMiddleClick = () => {
    middleClickTarget = null;
    if (resetTimer !== null) {
      clearTimeout(resetTimer);
      resetTimer = null;
    }
  };
  const handlePointerDown = (event: PointerEvent) => {
    clearMiddleClick();
    if (event.button === 1 && event.target instanceof Node) {
      middleClickTarget = event.target;
    }
  };
  const handleAuxClick = (event: MouseEvent) => {
    if (event.button !== 1) {
      return;
    }

    resetTimer = setTimeout(clearMiddleClick, 0);
  };
  const handlePaste = (event: ClipboardEvent) => {
    if (!(event.target instanceof Element)) {
      return;
    }

    const editor = event.target.closest(EDITOR_SELECTOR);
    if (!editor || !middleClickTarget || editor.contains(middleClickTarget)) {
      return;
    }

    clearMiddleClick();
    event.preventDefault();
    event.stopImmediatePropagation();
  };

  document.addEventListener("pointerdown", handlePointerDown, true);
  document.addEventListener("pointercancel", clearMiddleClick, true);
  document.addEventListener("auxclick", handleAuxClick, true);
  document.addEventListener("paste", handlePaste, true);
  document.defaultView?.addEventListener("blur", clearMiddleClick);

  return () => {
    clearMiddleClick();
    document.removeEventListener("pointerdown", handlePointerDown, true);
    document.removeEventListener("pointercancel", clearMiddleClick, true);
    document.removeEventListener("auxclick", handleAuxClick, true);
    document.removeEventListener("paste", handlePaste, true);
    document.defaultView?.removeEventListener("blur", clearMiddleClick);
  };
}
