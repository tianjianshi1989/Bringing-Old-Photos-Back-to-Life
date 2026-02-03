import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

type ModifyPhotoResult = {
  outputPath: string;
};

type ProgressEvent = {
  runId: string;
  stage: number | null;
  message: string;
  isError: boolean;
};

function el<T extends HTMLElement>(selector: string): T {
  const node = document.querySelector(selector);
  if (!node) {
    throw new Error(`Missing element: ${selector}`);
  }
  return node as T;
}

function setStatus(text: string) {
  el<HTMLElement>("#status-text").textContent = text;
}

function setPreview(img: HTMLImageElement, placeholder: HTMLElement, filePath: string | null) {
  if (!filePath) {
    img.removeAttribute("src");
    img.classList.remove("visible");
    placeholder.classList.add("visible");
    return;
  }
  img.src = `${convertFileSrc(filePath)}?t=${Date.now()}`;
  img.classList.add("visible");
  placeholder.classList.remove("visible");
}

function normalizePath(p: unknown): string | null {
  if (!p) return null;
  if (typeof p === "string") return p;
  return null;
}

window.addEventListener("DOMContentLoaded", () => {
  const inputPathEl = el<HTMLInputElement>("#input-path");
  const outputFolderEl = el<HTMLInputElement>("#output-folder");
  const withScratchEl = el<HTMLInputElement>("#opt-with-scratch");
  const hrEl = el<HTMLInputElement>("#opt-hr");
  const gpuEl = el<HTMLInputElement>("#gpu-ids");
  const pythonEl = el<HTMLInputElement>("#python-exe");
  const btnBrowse = el<HTMLButtonElement>("#btn-browse");
  const btnRun = el<HTMLButtonElement>("#btn-run");
  const inputImg = el<HTMLImageElement>("#img-input");
  const outputImg = el<HTMLImageElement>("#img-output");
  const inputPlaceholder = el<HTMLElement>("#placeholder-input");
  const outputPlaceholder = el<HTMLElement>("#placeholder-output");
  const progressFill = el<HTMLElement>("#progressFill");
  const progressText = el<HTMLElement>("#progressText");
  const logBox = el<HTMLPreElement>("#logBox");

  let selectedInputPath: string | null = null;
  let activeRunId: string | null = null;
  let logLines: string[] = [];
  let lastStage: number | null = null;
  let pendingLogLines: string[] = [];
  let flushTimer: number | null = null;

  function setProgress(stage: number | null, text: string) {
    const pct = stage == null ? 0 : Math.max(0, Math.min(4, stage)) * 25;
    progressFill.style.width = `${pct}%`;
    progressText.textContent = text;
  }

  function resetLog() {
    logLines = [];
    pendingLogLines = [];
    if (flushTimer != null) {
      window.clearTimeout(flushTimer);
      flushTimer = null;
    }
    logBox.textContent = "";
  }

  function appendLog(line: string) {
    pendingLogLines.push(line);
    if (flushTimer != null) return;
    flushTimer = window.setTimeout(() => {
      flushTimer = null;
      if (pendingLogLines.length === 0) return;
      logLines.push(...pendingLogLines);
      pendingLogLines = [];
      if (logLines.length > 400) {
        logLines = logLines.slice(logLines.length - 400);
      }
      logBox.textContent = logLines.join("\n");
      logBox.scrollTop = logBox.scrollHeight;
    }, 100);
  }

  listen<ProgressEvent>("modify_progress", (event) => {
    const payload = event.payload;
    if (!payload || !activeRunId || payload.runId !== activeRunId) return;
    if (payload.stage !== lastStage && payload.stage != null) {
      lastStage = payload.stage;
      if (payload.stage <= 0) setProgress(0, "Starting...");
      else if (payload.stage >= 4) setProgress(4, "Finishing...");
      else setProgress(payload.stage, `Stage ${payload.stage}/4`);
    }
    if (payload.message) {
      appendLog(payload.isError ? `[stderr] ${payload.message}` : payload.message);
    }
  }).catch((e) => {
    setStatus(`Error: ${String(e)}`);
  });

  async function pickInputFile() {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png", "bmp", "tif", "tiff"] }],
    });
    const path = normalizePath(picked);
    if (!path) return;
    selectedInputPath = path;
    inputPathEl.value = path;
    setPreview(inputImg, inputPlaceholder, path);
    setPreview(outputImg, outputPlaceholder, null);
    setStatus(`Selected: ${path.split("/").pop() ?? path}`);
  }

  async function runModify() {
    if (!selectedInputPath) {
      setStatus("Please choose an input file first.");
      return;
    }
    btnRun.disabled = true;
    activeRunId = `${Date.now()}_${Math.random().toString(16).slice(2)}`;
    lastStage = null;
    resetLog();
    setProgress(null, "Starting...");
    setStatus("Running...");
    setPreview(outputImg, outputPlaceholder, null);

    try {
      const outputFolder = outputFolderEl.value.trim() || null;
      const gpu = gpuEl.value.trim() || "-1";
      const python = pythonEl.value.trim() || "python3";
      const res = await invoke<ModifyPhotoResult>("modify_photo", {
        args: {
          runId: activeRunId,
          inputPath: selectedInputPath,
          outputFolder,
          gpu,
          withScratch: Boolean(withScratchEl.checked),
          hr: Boolean(hrEl.checked),
          python,
        },
      });
      setPreview(outputImg, outputPlaceholder, res.outputPath);
      setProgress(4, "Done");
      setStatus(`Done: ${res.outputPath.split("/").pop() ?? res.outputPath}`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setProgress(null, "Error");
      setStatus(`Error: ${msg}`);
    } finally {
      btnRun.disabled = false;
    }
  }

  btnBrowse.addEventListener("click", () => {
    pickInputFile().catch((e) => setStatus(`Error: ${String(e)}`));
  });
  btnRun.addEventListener("click", () => {
    runModify().catch((e) => setStatus(`Error: ${String(e)}`));
  });
});
