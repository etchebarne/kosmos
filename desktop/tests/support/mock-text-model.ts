import type { editor } from "monaco-editor";

type TextChange = { offset: number; length: number; text: string };

/**
 * A minimal stand-in for Monaco's text model: it applies offset edits, tracks the
 * version ids the editor buffers rely on, and returns Monaco-shaped change events.
 */
export class MockTextModel {
  private value: string;
  private versionId = 1;
  private alternativeVersionId = 1;
  private disposed = false;

  constructor(value: string) {
    this.value = value;
  }

  getValue(): string {
    return this.value;
  }

  getValueLength(): number {
    return this.value.length;
  }

  getVersionId(): number {
    return this.versionId;
  }

  getAlternativeVersionId(): number {
    return this.alternativeVersionId;
  }

  getFullModelRange(): { start: 0; end: number } {
    return { start: 0, end: this.value.length };
  }

  isDisposed(): boolean {
    return this.disposed;
  }

  dispose(): void {
    this.disposed = true;
  }

  setValue(value: string): editor.IModelContentChangedEvent {
    return this.edit({ offset: 0, length: this.value.length, text: value });
  }

  /** Applies one replacement and returns the change event Monaco would emit. */
  edit(change: TextChange): editor.IModelContentChangedEvent {
    const event = this.applyChange(change);
    return event as unknown as editor.IModelContentChangedEvent;
  }

  private applyChange(change: TextChange) {
    this.value =
      this.value.slice(0, change.offset) + change.text + this.value.slice(change.offset + change.length);
    this.versionId += 1;
    this.alternativeVersionId += 1;
    return {
      changes: [
        {
          range: undefined as never,
          rangeOffset: change.offset,
          rangeLength: change.length,
          text: change.text,
        },
      ],
      eol: "\n",
      versionId: this.versionId,
      isUndoing: false,
      isRedoing: false,
      isFlush: false,
      isEolChange: false,
    };
  }

  pushEditOperations(
    _selections: unknown,
    edits: Array<{ text: string | null }>,
  ): null {
    this.setValue(edits[0]?.text ?? "");
    return null;
  }

  asModel(): editor.ITextModel {
    return this as unknown as editor.ITextModel;
  }
}
