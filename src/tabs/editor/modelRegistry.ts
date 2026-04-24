import type { Monaco } from "@monaco-editor/react";
import type { editor } from "monaco-editor";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { TextDocumentSyncKind } from "vscode-languageserver-protocol";
import type { TextDocumentContentChangeEvent } from "vscode-languageserver-protocol";
import { useLayoutStore } from "../../store/layout.store";
import { useLspStore } from "../../store/lsp.store";
import { useSettingsStore } from "../../store/settings.store";
import { useToastStore } from "../../store/toast.store";
import type { Workspace } from "../../store/workspace.store";
import { normalizePath } from "../../lib/pathUtils";
import { pathToFileUri } from "../../lib/lsp/uri";
import { resolveModelLanguage } from "../../lib/lsp/monacoLanguages";
import { parseDiffChanges, buildDiffDecorations } from "./diffDecorations";
import {
  AI_GENERATE_GLYPH_LOADING_CLASS,
  buildAiGutterDecorations,
  buildGenerationPrompt,
  buildWindowedContext,
  extractFunctions,
  stripCodeFences,
  type AiFunctionInfo,
} from "./aiGutter";

const DIDCHANGE_DEBOUNCE_MS = 200;
const AI_REFRESH_DEBOUNCE_MS = 400;

export interface LspBinding {
  workspacePath: string;
  fileUri: string;
  lspLanguage: string;
}

type ContentChangeCallback = (e: editor.IModelContentChangedEvent) => void;

interface AiInFlight {
  glyphId: string;
  rangeId: string;
}

interface AiGeneratePayload {
  text: string;
  stderr: string;
  raw: string;
  tempPath: string;
}

export interface ModelRegistryEntry {
  filePath: string;
  model: editor.ITextModel;
  refCount: number;
  tabIds: Set<string>;
  /** alternativeVersionId that matches the on-disk content. */
  savedVersionId: number;
  /** True while the registry itself is mutating the model (external reload). */
  suppressForExternal: boolean;
  /** Current LSP binding — set once the file's LSP server is available. */
  lspBinding: LspBinding | null;
  lspOpened: boolean;
  lspVersion: number;
  pendingLspChanges: TextDocumentContentChangeEvent[];
  lspDebounceTimer: ReturnType<typeof setTimeout> | null;
  /** Extra subscribers, e.g. from SharedPaneEditor wiring. */
  changeSubscribers: Set<ContentChangeCallback>;
  /** Single Monaco subscription that drives the fanout. */
  disposable: { dispose: () => void };
  // AI gutter (file-level state)
  aiFunctions: Map<number, AiFunctionInfo>;
  aiGutterIds: string[];
  aiInFlight: Map<number, AiInFlight>;
  aiGenerationCounter: number;
  aiRefreshTimer: ReturnType<typeof setTimeout> | null;
  // Diff gutter
  diffIds: string[];
  /** Workspace used for the current diff decorations (so we can re-check on workspace change). */
  diffWorkspacePath: string | null;
}

// Keyed by normalized absolute path.
const registry = new Map<string, ModelRegistryEntry>();

function registryKey(filePath: string): string {
  return normalizePath(filePath);
}

export function getModelEntry(filePath: string): ModelRegistryEntry | undefined {
  return registry.get(registryKey(filePath));
}

// --- Registry events ----------------------------------------------------------------

type RegistryEventType = "model-created" | "model-disposed";
interface RegistryEvent {
  type: RegistryEventType;
  filePath: string;
}
const eventListeners = new Set<(e: RegistryEvent) => void>();

export function onRegistryEvent(cb: (e: RegistryEvent) => void): () => void {
  eventListeners.add(cb);
  return () => {
    eventListeners.delete(cb);
  };
}

function emitEvent(e: RegistryEvent) {
  for (const cb of eventListeners) {
    try {
      cb(e);
    } catch (err) {
      console.error("modelRegistry event listener threw:", err);
    }
  }
}

// --- Model acquire / release --------------------------------------------------------

/**
 * Acquire (or increment-ref) the registry entry for `filePath`. If no model exists
 * for the file's URI yet, one is created from `initialContent` and `languageId`.
 */
export function acquireModel(params: {
  tabId: string;
  filePath: string;
  monaco: Monaco;
  initialContent: string;
  languageId: string;
}): { entry: ModelRegistryEntry; release: () => void } {
  const { tabId, filePath, monaco, initialContent, languageId } = params;
  const key = registryKey(filePath);
  let entry = registry.get(key);
  let created = false;

  if (!entry) {
    const uri = monaco.Uri.parse(pathToFileUri(filePath));
    let model = monaco.editor.getModel(uri);
    if (!model) {
      model = monaco.editor.createModel(initialContent, languageId, uri);
    }
    // Narrow language (typescriptreact over typescript, etc.).
    resolveModelLanguage(monaco, model);

    entry = {
      filePath,
      model,
      refCount: 0,
      tabIds: new Set(),
      savedVersionId: model.getAlternativeVersionId(),
      suppressForExternal: false,
      lspBinding: null,
      lspOpened: false,
      lspVersion: 0,
      pendingLspChanges: [],
      lspDebounceTimer: null,
      changeSubscribers: new Set(),
      disposable: { dispose: () => {} },
      aiFunctions: new Map(),
      aiGutterIds: [],
      aiInFlight: new Map(),
      aiGenerationCounter: 0,
      aiRefreshTimer: null,
      diffIds: [],
      diffWorkspacePath: null,
    };

    const createdEntry: ModelRegistryEntry = entry;
    createdEntry.disposable = model.onDidChangeContent((e: editor.IModelContentChangedEvent) => {
      handleContentChange(createdEntry, e);
    });

    registry.set(key, entry);
    ensureExternalListener();
    ensureGitChangedListener();
    created = true;
  }

  entry.refCount++;
  entry.tabIds.add(tabId);

  if (created) emitEvent({ type: "model-created", filePath });

  let released = false;
  const release = () => {
    if (released) return;
    released = true;
    entry!.tabIds.delete(tabId);
    entry!.refCount--;

    if (entry!.refCount < 0) {
      console.warn(`modelRegistry: refcount below zero for ${filePath}`);
      entry!.refCount = 0;
    }

    useLayoutStore.getState().setTabDirty(tabId, false);

    if (entry!.refCount === 0) {
      disposeEntry(entry!);
      registry.delete(key);
      maybeStopExternalListener();
      maybeStopGitChangedListener();
      emitEvent({ type: "model-disposed", filePath });
    }
  };

  return { entry, release };
}

function disposeEntry(entry: ModelRegistryEntry) {
  if (entry.lspDebounceTimer != null) {
    clearTimeout(entry.lspDebounceTimer);
    entry.lspDebounceTimer = null;
  }
  if (entry.aiRefreshTimer != null) {
    clearTimeout(entry.aiRefreshTimer);
    entry.aiRefreshTimer = null;
  }

  const binding = entry.lspBinding;
  if (binding && entry.lspOpened) {
    if (entry.pendingLspChanges.length > 0) {
      const state = useLspStore.getState();
      const client = state.getClient(binding.workspacePath, binding.lspLanguage);
      if (client) {
        client.didChange(binding.fileUri, entry.lspVersion, entry.pendingLspChanges);
      }
      for (const companion of state.getCompanionClients(
        binding.workspacePath,
        binding.lspLanguage,
      )) {
        companion.didChange(binding.fileUri, entry.lspVersion, entry.pendingLspChanges);
      }
      entry.pendingLspChanges = [];
    }
    const state = useLspStore.getState();
    const client = state.getClient(binding.workspacePath, binding.lspLanguage);
    client?.didClose(binding.fileUri);
    for (const companion of state.getCompanionClients(binding.workspacePath, binding.lspLanguage)) {
      companion.didClose(binding.fileUri);
    }
  }

  entry.changeSubscribers.clear();
  entry.disposable.dispose();
  if (!entry.model.isDisposed()) {
    entry.model.dispose();
  }
}

// --- Content-change fanout ----------------------------------------------------------

function handleContentChange(entry: ModelRegistryEntry, e: editor.IModelContentChangedEvent) {
  if (entry.suppressForExternal) return;

  const vid = entry.model.getAlternativeVersionId();
  const shouldBeDirty = vid !== entry.savedVersionId;
  const store = useLayoutStore.getState();
  for (const tabId of entry.tabIds) {
    if (shouldBeDirty !== store.dirtyTabs.has(tabId)) {
      store.setTabDirty(tabId, shouldBeDirty);
    }
  }

  const binding = entry.lspBinding;
  if (binding) {
    queueLspDidChange(entry, binding, e);
  }

  // AI gutter symbols likely shifted; debounced re-fetch.
  scheduleAiRefresh(entry);

  for (const sub of entry.changeSubscribers) {
    try {
      sub(e);
    } catch (err) {
      console.error("modelRegistry change subscriber threw:", err);
    }
  }
}

function queueLspDidChange(
  entry: ModelRegistryEntry,
  binding: LspBinding,
  e: editor.IModelContentChangedEvent,
) {
  const lspState = useLspStore.getState();
  const client = lspState.getClient(binding.workspacePath, binding.lspLanguage);
  if (!client) return;

  entry.lspVersion++;

  const syncKind =
    typeof client.capabilities?.textDocumentSync === "object"
      ? client.capabilities.textDocumentSync.change
      : client.capabilities?.textDocumentSync;

  if (syncKind === TextDocumentSyncKind.Full) {
    entry.pendingLspChanges = [{ text: entry.model.getValue() }];
  } else {
    const changes = e.changes.map((change) => ({
      range: {
        start: {
          line: change.range.startLineNumber - 1,
          character: change.range.startColumn - 1,
        },
        end: {
          line: change.range.endLineNumber - 1,
          character: change.range.endColumn - 1,
        },
      },
      rangeLength: change.rangeLength,
      text: change.text,
    }));
    entry.pendingLspChanges.push(...changes);
  }

  if (entry.lspDebounceTimer != null) clearTimeout(entry.lspDebounceTimer);
  entry.lspDebounceTimer = setTimeout(() => {
    entry.lspDebounceTimer = null;
    if (entry.pendingLspChanges.length === 0) return;
    const bindingNow = entry.lspBinding;
    if (!bindingNow) return;
    const state = useLspStore.getState();
    const currentClient = state.getClient(bindingNow.workspacePath, bindingNow.lspLanguage);
    currentClient?.didChange(bindingNow.fileUri, entry.lspVersion, entry.pendingLspChanges);
    for (const companion of state.getCompanionClients(
      bindingNow.workspacePath,
      bindingNow.lspLanguage,
    )) {
      companion.didChange(bindingNow.fileUri, entry.lspVersion, entry.pendingLspChanges);
    }
    entry.pendingLspChanges = [];
  }, DIDCHANGE_DEBOUNCE_MS);
}

// --- LSP lifecycle helpers ----------------------------------------------------------

/**
 * Record the file's LSP binding and perform didOpen. Safe to call multiple times
 * — subsequent calls with the same binding are no-ops.
 */
export function openLspForModel(params: {
  filePath: string;
  workspacePath: string;
  fileUri: string;
  lspLanguage: string;
}): void {
  const entry = getModelEntry(params.filePath);
  if (!entry) return;

  const existing = entry.lspBinding;
  if (
    existing &&
    existing.workspacePath === params.workspacePath &&
    existing.fileUri === params.fileUri &&
    existing.lspLanguage === params.lspLanguage
  ) {
    return;
  }

  if (existing && entry.lspOpened) {
    const state = useLspStore.getState();
    const client = state.getClient(existing.workspacePath, existing.lspLanguage);
    client?.didClose(existing.fileUri);
    for (const companion of state.getCompanionClients(
      existing.workspacePath,
      existing.lspLanguage,
    )) {
      companion.didClose(existing.fileUri);
    }
    entry.lspOpened = false;
  }

  entry.lspBinding = {
    workspacePath: params.workspacePath,
    fileUri: params.fileUri,
    lspLanguage: params.lspLanguage,
  };

  const state = useLspStore.getState();
  const client = state.getClient(params.workspacePath, params.lspLanguage);
  if (!client) return;

  entry.lspVersion = 1;
  const text = entry.model.getValue();
  client.didOpen(params.fileUri, params.lspLanguage, entry.lspVersion, text);
  entry.lspOpened = true;

  for (const companion of state.getCompanionClients(params.workspacePath, params.lspLanguage)) {
    companion.didOpen(params.fileUri, params.lspLanguage, entry.lspVersion, text);
  }

  // LSP just came up; kick off symbol refresh so AI glyphs appear.
  scheduleAiRefresh(entry);
}

/**
 * Send didOpen to any companion LSP clients that spawned after the main didOpen.
 */
export function openLspCompanionsForModel(filePath: string): void {
  const entry = getModelEntry(filePath);
  if (!entry || !entry.lspBinding || !entry.lspOpened) return;
  const state = useLspStore.getState();
  const text = entry.model.getValue();
  for (const companion of state.getCompanionClients(
    entry.lspBinding.workspacePath,
    entry.lspBinding.lspLanguage,
  )) {
    companion.didOpen(
      entry.lspBinding.fileUri,
      entry.lspBinding.lspLanguage,
      entry.lspVersion,
      text,
    );
  }
}

/** Baseline the saved version (called after a successful disk write). */
export function setModelSavedVersion(filePath: string, versionId: number): void {
  const entry = getModelEntry(filePath);
  if (!entry) return;
  entry.savedVersionId = versionId;
  const store = useLayoutStore.getState();
  const stillDirty = entry.model.getAlternativeVersionId() !== versionId;
  for (const tabId of entry.tabIds) {
    if (stillDirty !== store.dirtyTabs.has(tabId)) {
      store.setTabDirty(tabId, stillDirty);
    }
  }
}

export function subscribeToModelChanges(filePath: string, cb: ContentChangeCallback): () => void {
  const entry = getModelEntry(filePath);
  if (!entry) return () => {};
  entry.changeSubscribers.add(cb);
  return () => {
    entry.changeSubscribers.delete(cb);
  };
}

// --- AI gutter (file-level) ---------------------------------------------------------

function rebuildAiDecorations(entry: ModelRegistryEntry) {
  const inFlightLines = new Set<number>();
  for (const { glyphId } of entry.aiInFlight.values()) {
    const r = entry.model.getDecorationRange(glyphId);
    if (r) inFlightLines.add(r.startLineNumber);
  }
  const decorations = buildAiGutterDecorations(entry.aiFunctions, inFlightLines);
  entry.aiGutterIds = entry.model.deltaDecorations(entry.aiGutterIds, decorations);
}

function scheduleAiRefresh(entry: ModelRegistryEntry) {
  if (entry.aiRefreshTimer != null) clearTimeout(entry.aiRefreshTimer);
  entry.aiRefreshTimer = setTimeout(() => {
    entry.aiRefreshTimer = null;
    refreshAiGutter(entry);
  }, AI_REFRESH_DEBOUNCE_MS);
}

async function refreshAiGutter(entry: ModelRegistryEntry) {
  const binding = entry.lspBinding;
  const enabled = useSettingsStore.getState().values["ai.enableCompletion"] === true;

  if (!enabled || !binding || !entry.lspOpened) {
    entry.aiGutterIds = entry.model.deltaDecorations(entry.aiGutterIds, []);
    entry.aiFunctions = new Map();
    return;
  }

  const client = useLspStore.getState().getClient(binding.workspacePath, binding.lspLanguage);
  if (!client) return;

  try {
    const symbols = await client.documentSymbol(binding.fileUri);
    entry.aiFunctions = extractFunctions(symbols);
    rebuildAiDecorations(entry);
  } catch {
    entry.aiGutterIds = entry.model.deltaDecorations(entry.aiGutterIds, []);
    entry.aiFunctions = new Map();
  }
}

export function handleAiGlyphClick(filePath: string, line: number, monaco: Monaco): void {
  const entry = getModelEntry(filePath);
  if (!entry) return;
  if (useSettingsStore.getState().values["ai.enableCompletion"] !== true) return;
  if (entry.aiFunctions.has(line)) {
    void generateFunctionAtLine(entry, line, monaco);
  }
}

async function generateFunctionAtLine(
  entry: ModelRegistryEntry,
  startLine: number,
  monaco: Monaco,
) {
  const model = entry.model;
  const info = entry.aiFunctions.get(startLine);
  if (!info) return;

  // Re-click on an in-flight generation cancels it.
  for (const [existingGenId, { glyphId }] of entry.aiInFlight.entries()) {
    const r = model.getDecorationRange(glyphId);
    if (r && r.startLineNumber === startLine) {
      invoke("ai_cancel", { cancelId: String(existingGenId) });
      return;
    }
  }

  const installedAgents = await invoke<string[]>("ai_installed_agents");
  if (installedAgents.length === 0) {
    useToastStore.getState().addToast({ type: "warning", message: "No coding agent was found" });
    return;
  }

  const ids = model.deltaDecorations(
    [],
    [
      {
        range: {
          startLineNumber: info.range.startLineNumber,
          startColumn: 1,
          endLineNumber: info.range.startLineNumber,
          endColumn: 1,
        },
        options: {
          stickiness: monaco.editor.TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges,
          glyphMarginClassName: AI_GENERATE_GLYPH_LOADING_CLASS,
          glyphMarginHoverMessage: { value: "Click to cancel" },
        },
      },
      {
        range: info.range,
        options: {
          stickiness: monaco.editor.TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges,
        },
      },
    ],
  );

  const genId = ++entry.aiGenerationCounter;
  entry.aiInFlight.set(genId, { glyphId: ids[0], rangeId: ids[1] });
  rebuildAiDecorations(entry);

  try {
    const functionText = model.getValueInRange(info.range);
    const language = model.getLanguageId();
    const fp = entry.filePath;
    const context = buildWindowedContext(model, info.range);

    const prompt = buildGenerationPrompt({
      filePath: fp,
      language,
      context,
      functionText,
      functionStartLine: info.range.startLineNumber,
      functionEndLine: info.range.endLineNumber,
    });

    const settings = useSettingsStore.getState().values;
    const agent = (settings["ai.agent"] as string) ?? "claude-code";
    const agentModel =
      agent === "claude-code"
        ? ((settings["ai.claudeCode.model"] as string) ?? "sonnet")
        : agent === "codex"
          ? ((settings["ai.codex.model"] as string) ?? "gpt-5.3-codex")
          : null;
    const cwd = entry.lspBinding?.workspacePath ?? null;

    const result = await invoke<AiGeneratePayload>("ai_generate", {
      prompt,
      agent,
      model: agentModel,
      cwd,
      cancelId: String(genId),
    });

    console.groupCollapsed(`[ai_generate] line ${startLine}`);
    console.log("temp file:", result.tempPath);
    console.log("text (from temp file):", result.text);
    if (result.stderr.trim()) console.log("stderr:", result.stderr);
    if (result.raw.trim()) console.log("raw stdout (ignored):", result.raw);
    console.groupEnd();

    const cleaned = stripCodeFences(result.text).trimEnd();
    if (!cleaned) {
      console.warn(
        `[ai_generate] line ${startLine}: agent didn't write anything to the temp file, skipping edit`,
      );
      return;
    }
    if (cleaned === functionText.trimEnd()) {
      console.warn(`[ai_generate] line ${startLine}: response identical to source, skipping edit`);
      return;
    }

    if (model.isDisposed()) return;
    const currentRange = model.getDecorationRange(ids[1]);
    const target = currentRange ?? info.range;
    model.pushEditOperations(
      null,
      [{ range: target, text: cleaned, forceMoveMarkers: true }],
      () => null,
    );
  } catch (err) {
    if (String(err) !== "CANCELLED") {
      console.error("AI generation failed:", err);
    }
  } finally {
    if (!model.isDisposed()) {
      model.deltaDecorations([ids[0], ids[1]], []);
    }
    entry.aiInFlight.delete(genId);
    if (!model.isDisposed()) {
      rebuildAiDecorations(entry);
      scheduleAiRefresh(entry);
    }
  }
}

// Re-evaluate AI decorations when the completion toggle flips. Subscribes at module
// init; settings store outlives the registry so a one-shot subscription is fine.
let lastAiEnabled = useSettingsStore.getState().values["ai.enableCompletion"] === true;
useSettingsStore.subscribe((state) => {
  const next = state.values["ai.enableCompletion"] === true;
  if (next === lastAiEnabled) return;
  lastAiEnabled = next;
  for (const entry of registry.values()) {
    scheduleAiRefresh(entry);
  }
});

// --- Diff gutter (file-level) -------------------------------------------------------

export async function refreshDiffDecorations(
  filePath: string,
  workspace: Workspace | null,
): Promise<void> {
  const entry = getModelEntry(filePath);
  if (!entry) return;
  if (!workspace) {
    entry.diffIds = entry.model.deltaDecorations(entry.diffIds, []);
    entry.diffWorkspacePath = null;
    return;
  }

  const fp = entry.filePath;
  const relative = fp.startsWith(workspace.path + "/") ? fp.slice(workspace.path.length + 1) : fp;

  try {
    const patch = await invoke<string>("git_diff", {
      path: workspace.path,
      file: relative,
      staged: false,
    });
    if (entry.model.isDisposed()) return;
    const changes = parseDiffChanges(patch);
    const decorations = buildDiffDecorations(changes);
    entry.diffIds = entry.model.deltaDecorations(entry.diffIds, decorations);
    entry.diffWorkspacePath = workspace.path;
  } catch {
    if (entry.model.isDisposed()) return;
    entry.diffIds = entry.model.deltaDecorations(entry.diffIds, []);
    entry.diffWorkspacePath = workspace.path;
  }
}

// --- External file-change listener --------------------------------------------------

let externalUnlisten: Promise<() => void> | null = null;

function ensureExternalListener() {
  if (externalUnlisten) return;
  externalUnlisten = listen<string[]>("file-content-changed", (event) => {
    const changed = event.payload;
    const seen = new Set<ModelRegistryEntry>();
    for (const f of changed) {
      const e = registry.get(normalizePath(f));
      if (e) seen.add(e);
    }
    for (const entry of seen) {
      handleExternalFileChange(entry);
    }
  });
}

function maybeStopExternalListener() {
  if (registry.size > 0) return;
  const p = externalUnlisten;
  externalUnlisten = null;
  p?.then((fn) => fn());
}

async function handleExternalFileChange(entry: ModelRegistryEntry) {
  if (entry.model.getAlternativeVersionId() !== entry.savedVersionId) return;

  try {
    const newContent = await invoke<string>("read_file", { path: entry.filePath });
    if (entry.model.isDisposed()) return;
    if (newContent === entry.model.getValue()) return;

    entry.suppressForExternal = true;
    try {
      entry.model.setValue(newContent);
    } finally {
      entry.suppressForExternal = false;
    }
    entry.savedVersionId = entry.model.getAlternativeVersionId();
    const store = useLayoutStore.getState();
    for (const tabId of entry.tabIds) {
      if (store.dirtyTabs.has(tabId)) store.setTabDirty(tabId, false);
    }

    const binding = entry.lspBinding;
    if (binding && entry.lspOpened) {
      entry.lspVersion++;
      const state = useLspStore.getState();
      const client = state.getClient(binding.workspacePath, binding.lspLanguage);
      client?.didChange(binding.fileUri, entry.lspVersion, [{ text: newContent }]);
      for (const companion of state.getCompanionClients(
        binding.workspacePath,
        binding.lspLanguage,
      )) {
        companion.didChange(binding.fileUri, entry.lspVersion, [{ text: newContent }]);
      }
    }

    scheduleAiRefresh(entry);
  } catch {
    // File may have been deleted; leave the buffer alone.
  }
}

// --- git-changed listener (refreshes diff decorations for all entries) --------------

let gitChangedUnlisten: Promise<() => void> | null = null;

function ensureGitChangedListener() {
  if (gitChangedUnlisten) return;
  gitChangedUnlisten = listen("git-changed", () => {
    for (const entry of registry.values()) {
      if (!entry.diffWorkspacePath) continue;
      const ws = { path: entry.diffWorkspacePath } as Workspace;
      void refreshDiffDecorations(entry.filePath, ws);
    }
  });
}

function maybeStopGitChangedListener() {
  if (registry.size > 0) return;
  const p = gitChangedUnlisten;
  gitChangedUnlisten = null;
  p?.then((fn) => fn());
}
