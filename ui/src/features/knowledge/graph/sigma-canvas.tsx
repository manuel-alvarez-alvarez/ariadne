/**
 * The one place a knowledge graph meets sigma.js: the WebGL renderer, its
 * pointer events, the camera buttons, and the drag of a node.
 *
 * Everything that decides *what* is drawn — colours, emphasis, labels — is
 * `KnowledgeGraph`'s, handed down here as reducers, so this file only draws.
 * jsdom has no WebGL, so a test that draws a graph swaps this module for a stand-in that
 * lists the nodes and edges the reducers give (`@/test/sigma-canvas.tsx`).
 *
 * The graph arrives placed and settled — a force layout is a function that
 * ran before the first frame (`force-layout.ts`) — so this file starts no
 * worker and no timer, and a node moves only under the pointer that drags
 * it. sigma.js fits the graph to the view on its own; pan and zoom are its
 * mouse and trackpad gestures, and the buttons zoom and fit again.
 */

import "@react-sigma/core/lib/style.css"

import { SigmaContainer, useCamera, useRegisterEvents, useSigma } from "@react-sigma/core"
import { MaximizeIcon, ZoomInIcon, ZoomOutIcon } from "lucide-react"
import { useEffect, useMemo, useRef } from "react"
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
  /** Drawn above the rest, on the hover renderer. */
  highlighted: boolean
  /** Its label is drawn whatever the density of the labels around it. */
  forceLabel: boolean
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
  /** Its label is drawn even where the labels of its ends are not. */
  forceLabel: boolean
  hidden: boolean
  zIndex: number
}

interface SigmaCanvasProps {
  /**
   * Every node already placed and settled: see `placed` in `graph-model.ts`
   * and `forceLayout` in `force-layout.ts`.
   */
  graph: KnowledgeGraphModel
  /** What placed it, for whatever reads the view; the places are in the graph. */
  layout: GraphLayout
  palette: GraphPalette
  nodeReducer: (node: string, attributes: GraphNodeAttributes) => NodeDisplay
  edgeReducer: (edge: string, attributes: GraphEdgeAttributes) => EdgeDisplay
  onEnterNode: (node: string) => void
  onLeaveNode: () => void
  onClickNode: (node: string) => void
  onClickEdge: (edge: string) => void
}

/**
 * How thinly sigma.js picks the node labels it draws: at most one to a cell
 * of this many pixels, and none on a node drawn smaller than this.
 *
 * A graph of six hundred files has no room for six hundred names: what is
 * left is a grid of names far enough apart to read, more of them as the
 * camera zooms in. A hovered node, its neighbours and the edges between them
 * carry their labels whatever the density (`forceLabel`).
 */
const LABEL_DENSITY = 0.5
const LABEL_GRID_CELL_PX = 120
const LABEL_MIN_NODE_PX = 7

export function SigmaCanvas({ graph, palette, ...rest }: SigmaCanvasProps) {
  const settings = useMemo(
    (): Partial<Settings> => ({
      allowInvalidContainer: true,
      enableEdgeEvents: true,
      renderEdgeLabels: true,
      zIndex: true,
      labelDensity: LABEL_DENSITY,
      labelGridCellSize: LABEL_GRID_CELL_PX,
      labelRenderedSizeThreshold: LABEL_MIN_NODE_PX,
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
  nodeReducer,
  edgeReducer,
  onEnterNode,
  onLeaveNode,
  onClickNode,
  onClickEdge,
}: Omit<SigmaCanvasProps, "graph" | "palette" | "layout">) {
  const sigma = useSigma()
  const registerEvents = useRegisterEvents()
  /** The node under a pressed pointer, which every move writes a place to. */
  const dragged = useRef<string | null>(null)

  useEffect(() => {
    const pointer = (cursor: string) => {
      sigma.getContainer().style.cursor = cursor
    }
    const drop = () => {
      dragged.current = null
      pointer("")
    }
    // One call registers the lot: it replaces the handlers rather than adding
    // to them.
    registerEvents({
      enterNode: (event) => {
        pointer("pointer")
        onEnterNode(event.node)
      },
      leaveNode: () => {
        if (dragged.current === null) pointer("")
        onLeaveNode()
      },
      enterEdge: () => pointer("pointer"),
      leaveEdge: () => pointer(""),
      clickNode: (event) => onClickNode(event.node),
      clickEdge: (event) => onClickEdge(event.edge),
      downNode: (event) => {
        dragged.current = event.node
        pointer("grabbing")
        // The view would otherwise fit itself to the graph again at every
        // move, and the whole graph would slide under the dragged node.
        if (!sigma.getCustomBBox()) sigma.setCustomBBox(sigma.getBBox())
      },
      moveBody: ({ event }) => {
        const node = dragged.current
        if (node === null) return
        const place = sigma.viewportToGraph(event)
        sigma.getGraph().setNodeAttribute(node, "x", place.x)
        sigma.getGraph().setNodeAttribute(node, "y", place.y)
        // Without this the camera pans with the pointer as well.
        event.preventSigmaDefault()
        event.original.preventDefault()
        event.original.stopPropagation()
      },
      upNode: drop,
      upStage: drop,
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
