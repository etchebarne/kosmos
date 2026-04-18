import { registerTab } from "./registry";
import { blankTab } from "./blank";
import { fileTreeTab } from "./fileTree";
import { gitTab } from "./git";
import { editorTab } from "./editor";
import { changesTab } from "./changes";
import { terminalTab } from "./terminal";
import { settingsTab } from "./settings";
import { infinityTab } from "./infinity";
import { searchTab } from "./search";
import { marketplaceTab } from "./marketplace";

registerTab(blankTab);
registerTab(editorTab);
registerTab(changesTab);
registerTab(fileTreeTab);
registerTab(gitTab);
registerTab(terminalTab);
registerTab(infinityTab);
registerTab(searchTab);
registerTab(marketplaceTab);
registerTab(settingsTab);

export { getTabDefinition } from "./registry";
