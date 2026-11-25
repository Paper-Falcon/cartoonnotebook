import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";

// 手机端加载诊断：diag 条会轮询这个标志
(window as unknown as { __APP_BOOTED?: boolean }).__APP_BOOTED = true;

try {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
} catch (e) {
  const w = window as unknown as { __DIAG_LOG?: (m: string) => void };
  w.__DIAG_LOG?.(`渲染崩溃: ${e instanceof Error ? e.message : String(e)}`);
  throw e;
}
