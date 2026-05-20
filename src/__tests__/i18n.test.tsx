import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import App from "../App";
import { initI18n } from "../i18n";

describe("i18n bootstrap", () => {
  it("renders the PT-BR app title when locale is pt-BR", () => {
    initI18n("pt-BR");
    render(<App />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "VoiceTabs",
    );
    expect(screen.getByText("Anote falando.")).toBeInTheDocument();
  });

  it("switches to English when locale is en", () => {
    initI18n("en");
    render(<App />);
    expect(screen.getByText("Take notes by speaking.")).toBeInTheDocument();
  });
});
