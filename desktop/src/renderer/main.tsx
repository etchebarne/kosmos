import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { initializeLanguageClient } from "./lib/language-client";
import { initializeEditorBufferRecovery } from "./lib/editor-buffers";
import "./index.css";

document.documentElement.classList.add("dark");
initializeEditorBufferRecovery();
initializeLanguageClient();

const root = document.getElementById("root");

if (!root) {
  throw new Error("Missing root element");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
