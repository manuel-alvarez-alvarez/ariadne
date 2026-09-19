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
import type Graph from "graphology"
import forceAtlas2 from "graphology-layout-forceatlas2"
import FA2Layout from "graphology-layout-forceatlas2/worker"
import { MaximizeIcon, ZoomInIcon, ZoomOutIcon } from "lucide-react"
import { useEffect, useMemo } from "react"
import { drawDiscNodeLabel, EdgeRectangleProgram } from "sigma/rendering"
import type { Settings } from "sigma/settings"
import type { NodeDisplayData, PartialButFor } from "sigma/types"

import { Button } from "@/components/ui/button"

import { DashedEdgeProgram } from "./dashed-edge-program"
import {
  type GraphEdgeAttributes,
  type GraphLayout,
  type GraphNodeAttributes,
  type KnowledgeGraphModel,
  settled,
} from "./graph-model"
import type { GraphPalette } from "./graph-palette"

/** How one node is drawn. */
export interface NodeDisplay {
  label: string
  color: string
  size: number
  /** Drawn above the rest, with its label whatever the density. */
  highlighted: boolean
  /** Not drawn, and its edges with it. */
  hidden: boolean
  zIndex: number
}

/** How one edge is drawn. */
export interface EdgeDisplay {
  label: string
  color: string
  size: number
  type: "line" | "dashed"
  hidden: boolean
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

/** How often the force layout is checked for whether it has settled. */
const SETTLE_CHECK_MS = 250
/** How long the force layout may run at most, settled or not. */
const FORCE_LAYOUT_MS = 10_000
/** From this many nodes, the layout approximates far nodes in groups: exact is quadratic. */
const BARNES_HUT_NODES = 500

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
      getEdgeWeight: "weight",
      settings: {
        ...forceAtlas2.inferSettings(graph),
        gravity: 1,
        barnesHutOptimize: graph.order >= BARNES_HUT_NODES,
        edgeWeightInfluence: 1,
      },
    })
    worker.start()
    let last = positions(graph)
    const check = window.setInterval(() => {
      const next = positions(graph)
      if (settled(last, next)) stop()
      last = next
    }, SETTLE_CHECK_MS)
    const cap = window.setTimeout(() => stop(), FORCE_LAYOUT_MS)
    function stop() {
      window.clearInterval(check)
      window.clearTimeout(cap)
      worker.stop()
    }
    return () => {
      stop()
      worker.kill()
    }
  }, [sigma, layout])

  return null
}

/** Every node's place, as `x, y` pairs in node order. */
function positions(graph: Graph): Float64Array {
  const out = new Float64Array(graph.order * 2)
  let index = 0
  graph.forEachNode((_node, attributes) => {
    out[index++] = attributes.x ?? 0
    out[index++] = attributes.y ?? 0
  })
  return out
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
