import React from "react";
import ReactDOM from "react-dom/client";
import "../../styles/tailwind.css";
import { PopoverPanel } from "./PopoverPanel";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <PopoverPanel />
  </React.StrictMode>,
);
