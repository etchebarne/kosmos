import type { PaneId, SplitPaneId, WorkspaceId } from "./ids";
import type { TabSnapshot } from "./tab";

export type SplitAxis = "horizontal" | "vertical";

export type SplitPaneParams = {
  workspaceId?: WorkspaceId | null;
  paneId?: PaneId | null;
  axis: SplitAxis;
  newPaneFirst?: boolean;
};

export type ActivatePaneParams = {
  workspaceId?: WorkspaceId | null;
  paneId: PaneId;
};

export type MovePaneParams = {
  workspaceId?: WorkspaceId | null;
  paneId: PaneId;
  targetPaneId: PaneId;
  axis: SplitAxis;
  newPaneFirst?: boolean;
};

export type ResizeSplitParams = {
  workspaceId?: WorkspaceId | null;
  splitId: SplitPaneId;
  ratio: number;
};

export type PaneNodeSnapshot =
  | {
      type: "leaf";
      pane: PaneSnapshot;
    }
  | {
      type: "split";
      id: SplitPaneId;
      axis: SplitAxis;
      ratio: number;
      first: PaneNodeSnapshot;
      second: PaneNodeSnapshot;
    };

export type PaneSnapshot = {
  id: PaneId;
  activeTabId: number;
  tabs: TabSnapshot[];
};
