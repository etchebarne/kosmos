import { mkdtemp, readdir, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const desktopRoot = path.resolve(import.meta.dir, "..");
const generatedRoot = path.join(desktopRoot, "src", "shared", "ipc", "generated");
const expectedFiles = ["schema.json", "types.ts", "validators.ts"];
const temporaryRoot = await mkdtemp(path.join(os.tmpdir(), "kosmos-ipc-contract-"));

try {
  const process = Bun.spawn(
    ["bun", "run", "scripts/generate-ipc.ts", "--output", temporaryRoot],
    { cwd: desktopRoot, stderr: "inherit", stdout: "inherit" },
  );
  if ((await process.exited) !== 0) {
    throw new Error("IPC contract generation failed");
  }

  const trackedFiles = await readdir(generatedRoot);
  const extras = trackedFiles.filter((file) => !expectedFiles.includes(file));
  const missing = expectedFiles.filter((file) => !trackedFiles.includes(file));
  if (extras.length > 0 || missing.length > 0) {
    throw new Error(
      `IPC generated files differ: missing [${missing.join(", ")}], extra [${extras.join(", ")}]`,
    );
  }

  for (const file of expectedFiles) {
    const [tracked, generated] = await Promise.all([
      readFile(path.join(generatedRoot, file)),
      readFile(path.join(temporaryRoot, file)),
    ]);
    if (!tracked.equals(generated)) {
      throw new Error(`IPC generated file is stale: ${file}`);
    }
  }
} finally {
  await rm(temporaryRoot, { force: true, recursive: true });
}
