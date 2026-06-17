import React from "react";
import ReactDOM from "react-dom/client";
import { RunSummaryDiffWindow } from "./components/diff/RunSummaryDiffWindow";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <RunSummaryDiffWindow />
  </React.StrictMode>,
);
