/**
 * Markdown as the agents write it: task descriptions, conversation messages and
 * review bodies.
 *
 * No typography plugin is installed, so the element styles are spelled out
 * here — one place, so every markdown surface in the feature reads the same.
 */

import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"

import { cn } from "@/lib/utils"

const PROSE = [
  "text-sm leading-relaxed break-words",
  "[&>*+*]:mt-3",
  "[&_h1]:font-heading [&_h1]:text-base [&_h1]:font-semibold",
  "[&_h2]:font-heading [&_h2]:text-sm [&_h2]:font-semibold",
  "[&_h3]:font-heading [&_h3]:text-sm [&_h3]:font-medium",
  "[&_ul]:list-disc [&_ul]:pl-5 [&_ol]:list-decimal [&_ol]:pl-5 [&_li]:my-0.5",
  "[&_a]:text-primary [&_a]:underline [&_a]:underline-offset-3",
  "[&_code]:rounded [&_code]:bg-muted [&_code]:px-1 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-xs",
  "[&_pre]:overflow-x-auto [&_pre]:rounded-md [&_pre]:border [&_pre]:bg-muted/50 [&_pre]:p-3",
  "[&_pre_code]:bg-transparent [&_pre_code]:p-0",
  "[&_blockquote]:border-l-2 [&_blockquote]:pl-3 [&_blockquote]:text-muted-foreground",
  "[&_hr]:my-4 [&_hr]:border-t",
  "[&_table]:w-full [&_table]:text-left",
  "[&_th]:border-b [&_th]:py-1 [&_th]:pr-3 [&_th]:font-medium",
  "[&_td]:border-b [&_td]:py-1 [&_td]:pr-3 [&_td]:align-top",
].join(" ")

export function Markdown({ children, className }: { children: string; className?: string }) {
  return (
    <div className={cn(PROSE, className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          // Anything an agent links to is outside the app; keep the webview on
          // the app and hand the URL to the browser instead.
          a: ({ children: content, ...props }) => (
            <a {...props} target="_blank" rel="noreferrer noopener">
              {content}
            </a>
          ),
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  )
}
