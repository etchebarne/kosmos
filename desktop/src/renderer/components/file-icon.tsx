import {
  createFileTreeIconResolver,
  getBuiltInSpriteSheet,
} from "@pierre/trees";
import type { CSSProperties, SVGProps } from "react";

const FILE_TREE_ICON_SET = "complete";
const FILE_TREE_SPRITE_SHEET = getBuiltInSpriteSheet(FILE_TREE_ICON_SET);
const { resolveIcon } = createFileTreeIconResolver(FILE_TREE_ICON_SET);

const FILE_ICON_COLORS: Record<string, CSSProperties["color"]> = {
  astro: "var(--file-tree-icon-purple)",
  babel: "var(--file-tree-icon-yellow)",
  bash: "var(--file-tree-icon-green)",
  biome: "var(--file-tree-icon-blue)",
  bootstrap: "var(--file-tree-icon-indigo)",
  browserslist: "var(--file-tree-icon-yellow)",
  bun: "var(--file-tree-icon-mauve)",
  c: "var(--file-tree-icon-blue)",
  claude: "var(--file-tree-icon-orange)",
  cpp: "var(--file-tree-icon-blue)",
  css: "var(--file-tree-icon-indigo)",
  database: "var(--file-tree-icon-purple)",
  default: "var(--file-tree-icon-gray)",
  docker: "var(--file-tree-icon-blue)",
  eslint: "var(--file-tree-icon-indigo)",
  git: "var(--file-tree-icon-vermilion)",
  go: "var(--file-tree-icon-cyan)",
  graphql: "var(--file-tree-icon-pink)",
  html: "var(--file-tree-icon-orange)",
  image: "var(--file-tree-icon-pink)",
  javascript: "var(--file-tree-icon-yellow)",
  json: "var(--file-tree-icon-orange)",
  markdown: "var(--file-tree-icon-green)",
  mcp: "var(--file-tree-icon-teal)",
  npm: "var(--file-tree-icon-red)",
  oxc: "var(--file-tree-icon-cyan)",
  postcss: "var(--file-tree-icon-red)",
  prettier: "var(--file-tree-icon-teal)",
  python: "var(--file-tree-icon-blue)",
  react: "var(--file-tree-icon-cyan)",
  ruby: "var(--file-tree-icon-red)",
  rust: "var(--file-tree-icon-orange)",
  sass: "var(--file-tree-icon-pink)",
  svg: "var(--file-tree-icon-orange)",
  svelte: "var(--file-tree-icon-red)",
  svgo: "var(--file-tree-icon-green)",
  swift: "var(--file-tree-icon-orange)",
  table: "var(--file-tree-icon-teal)",
  tailwind: "var(--file-tree-icon-cyan)",
  terraform: "var(--file-tree-icon-indigo)",
  text: "var(--file-tree-icon-gray)",
  typescript: "var(--file-tree-icon-blue)",
  vite: "var(--file-tree-icon-purple)",
  vscode: "var(--file-tree-icon-blue)",
  vue: "var(--file-tree-icon-green)",
  wasm: "var(--file-tree-icon-indigo)",
  webpack: "var(--file-tree-icon-blue)",
  yml: "var(--file-tree-icon-red)",
  zig: "var(--file-tree-icon-orange)",
  zip: "var(--file-tree-icon-orange)",
};

type FileIconProps = Omit<SVGProps<SVGSVGElement>, "children"> & {
  path: string;
};

export function FileIcon({ path, style, ...props }: FileIconProps) {
  const icon = resolveIcon("file-tree-icon-file", path);

  return (
    <svg
      {...props}
      aria-hidden="true"
      data-icon-token={icon.token}
      focusable="false"
      style={{ color: fileIconColor(icon), ...style }}
      viewBox={icon.viewBox ?? `0 0 ${icon.width ?? 16} ${icon.height ?? 16}`}
    >
      <use href={`#${icon.name.replace(/^#/, "")}`} />
    </svg>
  );
}

export function FileIconSprite() {
  return (
    <span
      aria-hidden="true"
      className="pointer-events-none absolute size-0 overflow-hidden"
      dangerouslySetInnerHTML={{ __html: FILE_TREE_SPRITE_SHEET }}
    />
  );
}

function fileIconColor(icon: ReturnType<typeof resolveIcon>): CSSProperties["color"] {
  return icon.token ? FILE_ICON_COLORS[icon.token] : undefined;
}
