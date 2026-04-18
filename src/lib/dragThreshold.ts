export const DRAG_THRESHOLD = 5;

/**
 * Attach temporary mousemove/mouseup listeners that distinguish clicks from drags.
 * Call this from a mousedown handler.
 */
export function startDragThreshold(
  startX: number,
  startY: number,
  onDrag: () => void,
  onClickUp: () => void,
): void {
  let dragging = false;

  const onMouseMove = (ev: MouseEvent) => {
    if (dragging) return;
    const dx = ev.clientX - startX;
    const dy = ev.clientY - startY;
    if (Math.sqrt(dx * dx + dy * dy) > DRAG_THRESHOLD) {
      dragging = true;
      onDrag();
    }
  };

  const onMouseUp = () => {
    if (!dragging) {
      onClickUp();
    }
    document.removeEventListener("mousemove", onMouseMove);
    document.removeEventListener("mouseup", onMouseUp);
  };

  document.addEventListener("mousemove", onMouseMove);
  document.addEventListener("mouseup", onMouseUp);
}
