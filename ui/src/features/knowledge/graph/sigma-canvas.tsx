/**
 * The one place a knowledge graph meets sigma.js: the WebGL renderer, its
 * pointer events, the camera buttons, and the force layout.
 *
 * Everything that decides *what* is drawn — colours, emphasis, labels — is
 * `KnowledgeGraph`'s, handed down here as reducers, so this file only draws.
 * jsdom has no WebGL, so a test that draws a graph swaps this module for a stand-in that
 * lists the nodes and edges the reducers give (`@/test/sigma-canvas.tsx`).
 *
 * sigma.js fits the graph to the view on its own; pan and zoom are its mouse
 * and trackpad gestures, and the buttons zoom and fit again.
 */

import "@react-sigma/core/lib/style.css"

import { SigmaContainer, useCamera, useRegisterEvents, useSigma } from "@react-sigma/core"
import forceAtlas2 from "graphology-layout-forceatlas2"
import FA2Layout from "graphology-layout-forceatlas2/worker"
import { MaximizeIcon, ZoomInIcon, ZoomOutIcon } from "lucide-react"
import { useEffect, useMemo } from "react"
import { drawDiscNodeLabel, EdgeRectangleProgram } from "sigma/rendering"
import type { Settings } from "sigma/settings"
import type { NodeDisplayData, PartialButFor } from "sigma/types"

import { Button } from "@/components/ui/button"

import { DashedEdgeProgram } from "./dashed-edge-program"
import type {
  GraphEdgeAttributes,
  GraphLayout,
  GraphNodeAttributes,
  KnowledgeGraphModel,
} from "./graph-model"
import type { GraphPalette } from "./graph-palette"

/** How one node is drawn. */
export interface NodeDisplay {
  label: string
  color: string
  size: number
  /** Drawn above the rest, with its label whatever the density. */
  highlighted: boolean
  zIndex: number
}

/** How one edge is drawn. */
export interface EdgeDisplay {
  label: string
  color: string
  size: number
  type: "line" | "dashed"
  zIndex: number
}

export interface SigmaCanvasProps {
  /** Every node already placed: see `placed` in `graph-model.ts`. */
  graph: KnowledgeGraphModel
  layout: GraphLayout
  palette: GraphPalette
  nodeReducer: (node: string, attributes: GraphNodeAttributes) => NodeDisplay
  edgeReducer: (edge: string, attributes: GraphEdgeAttributes) => EdgeDisplay
  onEnterNode: (node: string) => void
  onLeaveNode: () => void
  onClickNode: (node: string) => void
  onClickEdge: (edge: string) => void
}

/** How long the force layout runs before it is stopped where it got to. */
const FORCE_LAYOUT_MS = 2000

export function SigmaCanvas({ graph, palette, ...rest }: SigmaCanvasProps) {
  const settings = useMemo(
    (): Partial<Settings> => ({
      allowInvalidContainer: true,
      enableEdgeEvents: true,
      renderEdgeLabels: true,
      zIndex: true,
      // Every label, however small the node: the graphs here are small, and a
      // node with no name says nothing.
      labelRenderedSizeThreshold: 0,
      labelFont: palette.font,
      labelSize: 12,
      labelWeight: "500",
      labelColor: { color: palette.label },
      edgeLabelFont: palette.font,
      edgeLabelSize: 11,
      edgeLabelColor: { color: palette.label },
      defaultEdgeType: "line",
      edgeProgramClasses: { line: EdgeRectangleProgram, dashed: DashedEdgeProgram },
      defaultDrawNodeHover: hoverDrawer(palette),
    }),
    [palette],
  )

  return (
    <SigmaContainer
      graph={graph}
      settings={settings}
      className="h-full w-full"
      // The package's stylesheet paints the canvas white; the card behind it is the background.
      style={{ background: "transparent" }}
    >
      <Behaviour {...rest} />
      <CameraButtons />
    </SigmaContainer>
  )
}

function Behaviour({
  layout,
  nodeReducer,
  edgeReducer,
  onEnterNode,
  onLeaveNode,
  onClickNode,
  onClickEdge,
}: Omit<SigmaCanvasProps, "graph" | "palette">) {
  const sigma = useSigma()
  const registerEvents = useRegisterEvents()

  useEffect(() => {
    const pointer = (cursor: string) => {
      sigma.getContainer().style.cursor = cursor
    }
    registerEvents({
      enterNode: (event) => {
        pointer("pointer")
        onEnterNode(event.node)
      },
      leaveNode: () => {
        pointer("")
        onLeaveNode()
      },
      enterEdge: () => pointer("pointer"),
      leaveEdge: () => pointer(""),
      clickNode: (event) => onClickNode(event.node),
      clickEdge: (event) => onClickEdge(event.edge),
    })
  }, [sigma, registerEvents, onEnterNode, onLeaveNode, onClickNode, onClickEdge])

  useEffect(() => {
    sigma.setSetting("nodeReducer", (node, data) => ({
      ...data,
      ...nodeReducer(node, data as unknown as GraphNodeAttributes),
    }))
    sigma.setSetting("edgeReducer", (edge, data) => ({
      ...data,
      ...edgeReducer(edge, data as unknown as GraphEdgeAttributes),
    }))
  }, [sigma, nodeReducer, edgeReducer])

  useEffect(() => {
    if (layout !== "force") return
    const graph = sigma.getGraph()
    if (graph.order < 2) return
    const worker = new FA2Layout(graph, {
      settings: { ...forceAtlas2.inferSettings(graph), gravity: 1, barnesHutOptimize: false },
    })
    worker.start()
    const stop = window.setTimeout(() => worker.stop(), FORCE_LAYOUT_MS)
    return () => {
      window.clearTimeout(stop)
      worker.kill()
    }
  }, [sigma, layout])

  return null
}

function CameraButtons() {
  const { zoomIn, zoomOut, reset } = useCamera({ duration: 200, factor: 1.5 })
  return (
    <div className="absolute top-2 right-2 flex flex-col gap-1">
      <Button variant="outline" size="icon-sm" aria-label="Zoom in" onClick={() => zoomIn()}>
        <ZoomInIcon />
      </Button>
      <Button variant="outline" size="icon-sm" aria-label="Zoom out" onClick={() => zoomOut()}>
        <ZoomOutIcon />
      </Button>
      <Button variant="outline" size="icon-sm" aria-label="Fit to view" onClick={() => reset()}>
        <MaximizeIcon />
      </Button>
    </div>
  )
}

/**
 * sigma.js draws a hovered node's label on a white box whatever the theme,
 * which in dark mode is light text on white: this draws the box in the
 * popover colour instead, then the label as sigma.js would.
 */
function hoverDrawer(palette: GraphPalette) {
  return (
    context: CanvasRenderingContext2D,
    data: PartialButFor<NodeDisplayData, "x" | "y" | "size" | "label" | "color">,
    settings: Settings,
  ) => {
    const size = settings.labelSize
    const padding = 3
    context.font = `${settings.labelWeight} ${size}px ${settings.labelFont}`
    context.fillStyle = palette.background
    context.shadowOffsetX = 0
    context.shadowOffsetY = 0
    context.shadowBlur = 8
    context.shadowColor = "#00000040"

    context.beginPath()
    context.arc(data.x, data.y, data.size + padding, 0, Math.PI * 2)
    if (typeof data.label === "string") {
      const width = context.measureText(data.label).width
      const height = size + 2 * padding
      context.rect(data.x, data.y - height / 2, data.size + width + 3 * padding, height)
    }
    context.fill()

    context.shadowBlur = 0
    context.shadowColor = "transparent"
    drawDiscNodeLabel(context, data, settings)
  }
}
