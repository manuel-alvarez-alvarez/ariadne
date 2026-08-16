/**
 * Markdown for goal descriptions and thread messages.
 *
 * Agents write markdown, so the thread has to render it. There is no typography
 * plugin in the stylesheet, so the handful of elements that actually show up in
 * agent prose are styled here instead of relying on `prose` classes.
 */

import ReactMarkdown, { type Components } from "react-markdown"
import remarkGfm from "remark-gfm"

import { cn } from "@/lib/utils"

const COMPONENTS: Components = {
  p: ({ children }) => <p className="leading-relaxed whitespace-pre-wrap">{children}</p>,
  a: ({ children, href }) => (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="text-primary underline underline-offset-3"
    >
      {children}
    </a>
  ),
  h1: ({ children }) => <h3 className="font-heading text-base font-semibold">{children}</h3>,
  h2: ({ children }) => <h4 className="font-heading text-sm font-semibold">{children}</h4>,
  h3: ({ children }) => <h5 className="font-heading text-sm font-semibold">{children}</h5>,
  ul: ({ children }) => <ul className="list-disc space-y-1 pl-5">{children}</ul>,
  ol: ({ children }) => <ol className="list-decimal space-y-1 pl-5">{children}</ol>,
  blockquote: ({ children }) => (
    <blockquote className="border-l-2 pl-3 text-muted-foreground">{children}</blockquote>
  ),
  code: ({ className, children }) => {
    // react-markdown gives fenced code a `language-*` class and inline code none.
    const fenced = typeof className === "string" && className.includes("language-")
    if (fenced) {
      return <code className="font-mono text-xs">{children}</code>
    }
    return <code className="rounded bg-muted px-1 py-0.5 font-mono text-[0.85em]">{children}</code>
  },
  pre: ({ children }) => (
    <pre className="overflow-x-auto rounded-md bg-muted p-3 text-xs">{children}</pre>
  ),
  table: ({ children }) => (
    <div className="overflow-x-auto">
      <table className="w-full border-collapse text-left">{children}</table>
    </div>
  ),
  th: ({ children }) => <th className="border-b px-2 py-1 font-medium">{children}</th>,
  td: ({ children }) => <td className="border-b px-2 py-1 align-top">{children}</td>,
  hr: () => <hr className="border-border" />,
}

export function Markdown({ children, className }: { children: string; className?: string }) {
  return (
    <div className={cn("space-y-3 text-sm break-words", className)}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={COMPONENTS}>
        {children}
      </ReactMarkdown>
    </div>
  )
}
