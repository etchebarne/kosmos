import type { StagedWorkspaceEdit } from "@/shared/ipc";

export type WorkspaceEditModelLineage<T> = {
  workspaceId: number;
  path: string;
  content: string;
  savedContent: string;
  value: T;
};

export type WorkspaceEditModelOutcome<T> = WorkspaceEditModelLineage<T> & {
  finalPath: string | null;
  finalContent: string;
};

type VirtualLineage<T> = WorkspaceEditModelOutcome<T> & {
  expectedContent: string;
  touched: boolean;
};

export function planWorkspaceEditModelLineages<T>(
  edit: StagedWorkspaceEdit,
  models: WorkspaceEditModelLineage<T>[],
): WorkspaceEditModelOutcome<T>[] {
  const lineages = models.map<VirtualLineage<T>>((model) => ({
    ...model,
    finalPath: model.path,
    finalContent: model.content,
    expectedContent: model.savedContent,
    touched: false,
  }));

  for (const operation of edit.operations) {
    if (operation.kind === "textDocument") {
      const document = edit.documents[operation.document];
      if (!document) {
        throw new Error(`Workspace edit document ${operation.document} is missing.`);
      }
      const targets = lineagesAt(lineages, document.workspaceId, document.path, false);
      if (document.generation === null || document.version === null) {
        if (targets.length > 0) {
          throw new Error(`Workspace edit target ${document.path} opened after validation.`);
        }
        continue;
      }
      if (targets.length === 0) {
        throw new Error(`Workspace edit target ${document.path} is not available.`);
      }
      for (const target of targets) {
        if (target.finalContent !== document.originalText) {
          throw new Error(`Workspace edit target ${document.path} has conflicting ordered edits.`);
        }
        target.finalContent = document.newText;
        target.expectedContent = document.newText;
        target.touched = true;
      }
      continue;
    }

    if (operation.kind === "renameFile") {
      const sources = lineagesAt(lineages, operation.workspaceId, operation.oldPath, true);
      const destinations = lineagesAt(
        lineages,
        operation.workspaceId,
        operation.newPath,
        true,
      );
      assertExpectedVirtualContent(destinations, "overwrite");
      for (const destination of destinations) {
        destination.finalPath = null;
        destination.touched = true;
      }
      for (const source of sources) {
        const suffix = pathSuffix(source.finalPath!, operation.oldPath);
        source.finalPath = joinPath(operation.newPath, suffix);
        source.touched = true;
      }
      continue;
    }

    const affected = lineagesAt(lineages, operation.workspaceId, operation.path, true);
    assertExpectedVirtualContent(
      affected,
      operation.kind === "deleteFile" ? "delete" : "overwrite",
    );
    for (const lineage of affected) {
      lineage.finalPath = null;
      lineage.touched = true;
    }
  }

  return lineages
    .filter((lineage) =>
      lineage.touched &&
      (lineage.finalPath !== lineage.path || lineage.finalContent !== lineage.content)
    )
    .map(({ expectedContent: _expectedContent, touched: _touched, ...outcome }) => outcome);
}

function lineagesAt<T>(
  lineages: VirtualLineage<T>[],
  workspaceId: number,
  path: string,
  descendants: boolean,
): VirtualLineage<T>[] {
  return lineages.filter((lineage) =>
    lineage.workspaceId === workspaceId &&
    lineage.finalPath !== null &&
    (lineage.finalPath === path ||
      (descendants && lineage.finalPath.startsWith(`${path}/`)))
  );
}

function assertExpectedVirtualContent<T>(
  lineages: VirtualLineage<T>[],
  operation: "delete" | "overwrite",
): void {
  for (const lineage of lineages) {
    if (lineage.finalContent !== lineage.expectedContent) {
      throw new Error(`Cannot ${operation} dirty open document ${lineage.finalPath}.`);
    }
  }
}

function pathSuffix(path: string, parent: string): string {
  return path === parent ? "" : path.slice(parent.length + 1);
}

function joinPath(parent: string, suffix: string): string {
  return suffix ? `${parent}/${suffix}` : parent;
}
