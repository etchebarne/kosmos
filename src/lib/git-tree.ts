export interface GitFileChange {
  path: string;
  status: string;
  staged: boolean;
  additions: number;
  deletions: number;
}

export interface GitStatusInfo {
  changes: GitFileChange[];
  branch: string | null;
  remoteBranch: string | null;
  lastCommitMessage: string | null;
  hasRemote: boolean;
  isRepo: boolean;
  ahead: number;
  behind: number;
}

export interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  children: TreeNode[];
  change?: GitFileChange;
}

export function buildChangeTree(changes: GitFileChange[]): TreeNode[] {
  if (changes.length === 0) return [];

  // Build TreeNode tree directly (no intermediate Trie)
  const root: TreeNode = { name: "", path: "", isDir: true, children: [] };

  for (const change of changes) {
    const parts = change.path.split("/");
    let current = root;

    for (let i = 0; i < parts.length; i++) {
      const isLast = i === parts.length - 1;

      if (isLast) {
        current.children.push({
          name: parts[i],
          path: change.path,
          isDir: false,
          children: [],
          change,
        });
      } else {
        let child = current.children.find((c) => c.isDir && c.name === parts[i]);
        if (!child) {
          child = { name: parts[i], path: parts[i], isDir: true, children: [] };
          current.children.push(child);
        }
        current = child;
      }
    }
  }

  // Collapse single-child directory chains and sort
  function collapseAndSort(nodes: TreeNode[]): TreeNode[] {
    for (const node of nodes) {
      if (!node.isDir) continue;

      // Collapse: dir with a single dir child merges names
      while (node.children.length === 1 && node.children[0].isDir) {
        const only = node.children[0];
        node.name += "/" + only.name;
        node.path = node.name;
        node.children = only.children;
      }

      collapseAndSort(node.children);
    }

    nodes.sort((a, b) => {
      if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
      return a.name.localeCompare(b.name);
    });

    return nodes;
  }

  return collapseAndSort(root.children);
}

export function getNodeFiles(node: TreeNode): GitFileChange[] {
  if (!node.isDir && node.change) return [node.change];
  return node.children.flatMap(getNodeFiles);
}
