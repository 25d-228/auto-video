import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import "./discover-category.css";

const applicationRoot = document.getElementById("root");

if (!applicationRoot) {
  throw new Error("Application root element was not found.");
}

createRoot(applicationRoot).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
