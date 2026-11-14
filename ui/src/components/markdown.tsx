/**
 * Markdown as the agents write it: goal and task descriptions, review bodies
 * and profile prompts — one renderer, so the same prose reads the same
 * wherever it is shown.
 *
 * No typography plugin is installed, so the element styles are spelled out
 * here, as variants on the wrapper rather than as element overrides: the list
 * then covers every element `remark-gfm` can produce, not only the ones that
 * were thought of.
 */

import { Children, isValidElement, type ReactNode } from "react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"

import { CodeBlock } from "@/components/code-block"
import { cn } from "@/lib/format"

const PROSE = [
  "text-sm leading-relaxed break-words",
  "[&>*+*]:mt-3",
  // Agents write prose with meaningful line breaks inside a paragraph.
  "[&_p]:whitespace-pre-wrap",
  "[&_h1]:font-heading [&_h1]:text-xl [&_h1]:leading-snug [&_h1]:font-semibold",
  "[&_h2]:font-heading [&_h2]:text-lg [&_h2]:leading-snug [&_h2]:font-semibold",
  "[&_h3]:font-heading [&_h3]:text-base [&_h3]:leading-snug [&_h3]:font-medium",
  // A nested list keeps its own indent; a task list drops its bullets, not its indent.
  "[&_ul]:list-disc [&_ul]:pl-5 [&_ol]:list-decimal [&_ol]:pl-5 [&_li]:my-0.5",
  "[&_ul.contains-task-list]:list-none [&_li.task-list-item_input]:mr-1.5 [&_li.task-list-item_input]:align-middle",
  "[&_del]:text-muted-foreground [&_del]:line-through",
  "[&_kbd]:rounded [&_kbd]:border [&_kbd]:bg-muted [&_kbd]:px-1 [&_kbd]:py-0.5 [&_kbd]:font-mono [&_kbd]:text-xs",
  "[&_a]:text-primary [&_a]:underline [&_a]:underline-offset-3",
  "[&_code]:rounded [&_code]:bg-muted [&_code]:px-1 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-xs",
  "[&_pre_code]:bg-transparent [&_pre_code]:p-0",
  "[&_blockquote]:border-l-2 [&_blockquote]:pl-3 [&_blockquote]:text-muted-foreground",
  "[&_hr]:my-4 [&_hr]:border-t",
  "[&_table]:w-full [&_table]:border-collapse [&_table]:text-left",
  "[&_th]:border-b [&_th]:py-1 [&_th]:pr-3 [&_th]:font-medium [&_th]:[overflow-wrap:break-word]",
  "[&_td]:border-b [&_td]:py-1 [&_td]:pr-3 [&_td]:align-top [&_td]:[overflow-wrap:anywhere]",
  "[&_img]:max-w-full",
].join(" ")

type Node = { type: string; value?: string; children?: Node[]; data?: { hName: string } }

/**
 * Raw HTML stays off, but a person writes `<kbd>Ctrl</kbd>` for a key: the
 * parser hands that over as an html node, the text, and a closing html node,
 * and this turns each such run into a `kbd` element.
 */
function remarkKbd() {
  const walk = (node: Node) => {
    const kids = node.children
    if (!kids) return
    for (let open = 0; open < kids.length; open += 1) {
      if (kids[open]?.type !== "html" || kids[open]?.value?.toLowerCase() !== "<kbd>") continue
      const close = kids.findIndex(
        (kid, at) => at > open && kid.type === "html" && kid.value?.toLowerCase() === "</kbd>",
      )
      if (close < 0) continue
      const inside = kids.slice(open + 1, close)
      kids.splice(open, close - open + 1, {
        type: "kbd",
        data: { hName: "kbd" },
        children: inside,
      })
    }
    kids.forEach(walk)
  }
  return walk
}

/** The text of a fenced block's `code` child, and the language its fence names. */
function fence(children: ReactNode): { code: string; language: string | undefined } | undefined {
  const child = Children.toArray(children)[0]
  if (!isValidElement<{ className?: string; children?: ReactNode }>(child)) return undefined
  const code = Children.toArray(child.props.children).join("").replace(/\n$/, "")
  return { code, language: /language-([\w+#-]+)/.exec(child.props.className ?? "")?.[1] }
}

export function Markdown({ children, className }: { children: string; className?: string }) {
  return (
    <div className={cn(PROSE, className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkKbd]}
        components={{
          // A fenced block is code: highlighted where the language is known,
          // with its Copy control, and never a scroll area of its own.
          pre: ({ children: content }) => {
            const block = fence(content)
            return block ? (
              <CodeBlock code={block.code} language={block.language} />
            ) : (
              <pre>{content}</pre>
            )
          },
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
