// View: render an oaudit spec doc (markdown) in the browser.
//
// Receives `data.markdown` (string) and `data.title` (string, optional).

import ReactMarkdown from "react-markdown";
import type { ViewProps } from "ui-leaf/view";

interface SpecData {
  markdown: string;
  title?: string;
}

export default function Spec({ data }: ViewProps<SpecData>) {
  return (
    <div
      style={{
        fontFamily: "system-ui, -apple-system, sans-serif",
        maxWidth: "48rem",
        margin: "2rem auto",
        padding: "0 1.5rem",
        color: "#1a1a1a",
        lineHeight: 1.55,
      }}
    >
      {data.title ? (
        <div
          style={{
            fontSize: "0.85rem",
            color: "#888",
            textTransform: "uppercase",
            letterSpacing: "0.06em",
            marginBottom: "1rem",
          }}
        >
          {data.title}
        </div>
      ) : null}
      <ReactMarkdown>{data.markdown}</ReactMarkdown>
    </div>
  );
}
