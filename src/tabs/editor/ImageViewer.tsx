import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { StateView } from "../../components/shared/StateView";
import { normalizePath } from "../../lib/pathUtils";

interface ImageViewerProps {
  filePath: string;
}

export function ImageViewer({ filePath }: ImageViewerProps) {
  // Cache-bust so external writes (save from Figma, git checkout, etc.) reload.
  const [cacheKey, setCacheKey] = useState(() => Date.now());
  const [broken, setBroken] = useState(false);

  useEffect(() => {
    setBroken(false);
    setCacheKey(Date.now());
  }, [filePath]);

  useEffect(() => {
    const unlisten = listen<string[]>("file-content-changed", (event) => {
      const normFilePath = normalizePath(filePath);
      if (event.payload.some((f) => normalizePath(f) === normFilePath)) {
        setBroken(false);
        setCacheKey(Date.now());
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [filePath]);

  const src = `${convertFileSrc(filePath)}?v=${cacheKey}`;

  if (broken) {
    return <StateView message="Failed to load image" variant="error" />;
  }

  return (
    <div className="flex items-center justify-center h-full w-full overflow-auto bg-[var(--color-bg-page)]">
      <img
        src={src}
        alt={filePath}
        className="max-w-full max-h-full object-contain"
        onError={() => setBroken(true)}
        draggable={false}
      />
    </div>
  );
}
