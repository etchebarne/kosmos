/**
 * On Linux, middle-click pastes the PRIMARY selection. When the user middle-drags
 * to select text, mouseup would otherwise paste and overwrite the fresh selection.
 * This guard suppresses paste only when a middle-drag occurred; plain middle-click
 * still pastes as expected.
 */
export function attachMiddleClickPasteGuard(editorDom: HTMLElement): () => void {
  const DRAG_THRESHOLD = 3;
  let middleDragged = false;
  let downX = 0;
  let downY = 0;

  const onMouseDown = (e: MouseEvent) => {
    if (e.button !== 1) return;
    middleDragged = false;
    downX = e.clientX;
    downY = e.clientY;
  };
  const onMouseMove = (e: MouseEvent) => {
    if (!(e.buttons & 4)) return;
    if (
      Math.abs(e.clientX - downX) > DRAG_THRESHOLD ||
      Math.abs(e.clientY - downY) > DRAG_THRESHOLD
    ) {
      middleDragged = true;
    }
  };
  const onMouseUp = (e: MouseEvent) => {
    if (e.button !== 1 || !middleDragged) return;
    e.preventDefault();
    e.stopPropagation();
  };

  editorDom.addEventListener("mousedown", onMouseDown, true);
  editorDom.addEventListener("mousemove", onMouseMove, true);
  editorDom.addEventListener("mouseup", onMouseUp, true);
  editorDom.addEventListener("auxclick", onMouseUp, true);

  return () => {
    editorDom.removeEventListener("mousedown", onMouseDown, true);
    editorDom.removeEventListener("mousemove", onMouseMove, true);
    editorDom.removeEventListener("mouseup", onMouseUp, true);
    editorDom.removeEventListener("auxclick", onMouseUp, true);
  };
}
