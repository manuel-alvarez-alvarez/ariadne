/**
 * The one graph view the knowledge screen draws with (022): a graphology
 * graph in, a zoomable, pannable sigma.js view out, with a legend under it.
 *
 * Every tab that shows a graph builds a {@link KnowledgeGraphModel} and hands
 * it here, so they all hover, click, fit and colour the same way. A node and
 * an edge name a tone, not a colour: the tones are read off the app's tokens
 * for the theme on screen (`graph-palette.ts`), and read again when it
 * changes.
 *
 * Hovering a node keeps it and its neighbours and fades the rest. A click on
 * a node or an edge goes to the caller, by its key in the graph. What
 * `hidden` names is left out of the drawing, and out of the model's layout
 * never: a filter redraws, and does not place the nodes again.
 */

import { useTheme } from "next-themes"
import { useCallback, useMemo, useState } from "react"

import { cn } from "@/lib/format"

import {
  emphasis,
  type GraphEdgeAttributes,
  type GraphLayout,
  type GraphNodeAttributes,
  type GraphVisibility,
  type KnowledgeGraphModel,
  type LegendEntry,
  placed,
} from "./graph-model"
import { type GraphPalette, graphPalette } from "./graph-palette"
import { type EdgeDisplay, type NodeDisplay, SigmaCanvas } from "./sigma-canvas"

const NODE_SIZE = 10
const EDGE_SIZE = 2
const NOTHING_HIDDEN: GraphVisibility = { nodes: new Set(), edges: new Set() }

export function KnowledgeGraph({
  graph,
  layout,
  legend,
  label,
  hidden = NOTHING_HIDDEN,
  onNodeClick,
  onEdgeClick,
  className,
}: {
  graph: KnowledgeGraphModel
  layout: GraphLayout
  legend: LegendEntry[]
  /** What the graph shows, for a screen reader: the canvas itself says nothing. */
  label: string
  hidden?: GraphVisibility
  onNodeClick?: (node: string) => void
  onEdgeClick?: (edge: string) => void
  className?: string
}) {
  const { resolvedTheme } = useTheme()
  // biome-ignore lint/correctness/useExhaustiveDependencies: the tokens change with the theme, which is not an argument
  const palette = useMemo(() => graphPalette(), [resolvedTheme])
  const shown = useMemo(() => placed(graph), [graph])
  const [hovered, setHovered] = useState<string | null>(null)
  const emphasised = useMemo(() => emphasis(shown, hovered), [shown, hovered])

  const nodeReducer = useCallback(
    (node: string, attributes: GraphNodeAttributes): NodeDisplay => {
      const { highlighted, faded } = emphasised.node(node)
      return {
        label: faded ? "" : attributes.label,
        color: faded ? palette.faded : palette.tones[attributes.tone],
        size: attributes.size ?? NODE_SIZE,
        highlighted,
        hidden: hidden.nodes.has(node),
        zIndex: highlighted ? 1 : 0,
      }
    },
    [emphasised, palette, hidden],
  )
  const edgeReducer = useCallback(
    (edge: string, attributes: GraphEdgeAttributes): EdgeDisplay => {
      const { highlighted, faded } = emphasised.edge(edge)
      return {
        label: faded ? "" : (attributes.label ?? ""),
        color: faded ? palette.faded : palette.tones[attributes.tone],
        size: attributes.size ?? EDGE_SIZE,
        type: attributes.dashed ? "dashed" : "line",
        hidden: hidden.edges.has(edge),
        zIndex: highlighted ? 1 : 0,
      }
    },
    [emphasised, palette, hidden],
  )
  const leave = useCallback(() => setHovered(null), [])
  const clickNode = useCallback((node: string) => onNodeClick?.(node), [onNodeClick])
  const clickEdge = useCallback((edge: string) => onEdgeClick?.(edge), [onEdgeClick])

  return (
    <div className={cn("flex min-h-0 flex-col gap-2", className)}>
      <section
        aria-label={label}
        className="relative min-h-80 flex-1 overflow-hidden rounded-lg border bg-card"
      >
        <SigmaCanvas
          graph={shown}
          layout={layout}
          palette={palette}
          nodeReducer={nodeReducer}
          edgeReducer={edgeReducer}
          onEnterNode={setHovered}
          onLeaveNode={leave}
          onClickNode={clickNode}
          onClickEdge={clickEdge}
        />
      </section>
      {legend.length > 0 ? <Legend entries={legend} palette={palette} /> : null}
    </div>
  )
}

function Legend({ entries, palette }: { entries: LegendEntry[]; palette: GraphPalette }) {
  return (
    <ul
      aria-label="Legend"
      className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground"
    >
      {entries.map((entry) => (
        <li key={entry.label} className="flex items-center gap-1.5">
          {entry.dashed ? (
            <span
              aria-hidden
              data-swatch={palette.tones[entry.tone]}
              className="w-4 border-t-2 border-dashed"
              style={{ borderColor: palette.tones[entry.tone] }}
            />
          ) : (
            <span
              aria-hidden
              data-swatch={palette.tones[entry.tone]}
              className="size-2.5 rounded-full"
              style={{ backgroundColor: palette.tones[entry.tone] }}
            />
          )}
          {entry.label}
        </li>
      ))}
    </ul>
  )
}
