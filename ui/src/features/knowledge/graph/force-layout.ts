/**
 * Where a force graph's nodes sit, decided once, before the first frame.
 *
 * ForceAtlas2 used to run in a worker while the view drew every step it
 * took, so a graph trembled for ten seconds and settled late or never: the
 * pull of an edge was its raw count, and a pair of files that share some
 * hundreds of edges swung the graph about. Here the layout is a function
 * instead. It seeds every node, runs a fixed number of iterations, and hands
 * back a graph that is already settled, so nothing moves after the first
 * frame but what the pointer drags.
 *
 * Two rules keep it still. An edge pulls with a damped weight, so one
 * thousand edges pull about ten times as hard as one, not a thousand times.
 * And the same graph gives the same positions: the seed is a spiral in node
 * order, and ForceAtlas2 draws no random number.
 */

import forceAtlas2, { type ForceAtlas2Settings } from "graphology-layout-forceatlas2"

import {
  type GraphEdgeAttributes,
  type GraphNodeAttributes,
  type KnowledgeGraphModel,
  NODE_SIZE,
} from "./graph-model"

/** From this many nodes, the layout groups far nodes: exact repulsion is quadratic. */
const BARNES_HUT_NODES = 500
/** How many iterations find the shape of the graph. */
const ITERATIONS = 300
/** How hard nodes push each other apart: the bigger it is, the wider the graph. */
const SCALING_RATIO = 40
/** How much of a step an iteration takes: the bigger it is, the calmer it ends. */
const SLOW_DOWN = 1
/** What sigma keeps clear of each side of its canvas: its `stagePadding`. */
const STAGE_PADDING_PX = 30
/**
 * How wide the narrowest canvas a force graph is drawn on is, in pixels.
 *
 * The narrowest one is the window at the width the side pane opens beside
 * the graph, `lg`, 1024 pixels: the sidebar takes 224 of those, the padding
 * of the main area takes 48, the pane takes 384, the gap beside it takes 12,
 * and the card's border takes 2. A narrower window falls under that
 * breakpoint, which stacks the pane under the graph and hands the graph the
 * whole row; a wider one hands it every pixel it gains. A railed sidebar
 * gives it 168 more.
 */
const VIEW_WIDTH_PX = 1024 - 224 - 48 - 384 - 12 - 2
/**
 * How wide the graph is handed back, in pixels: what sigma has left of that
 * canvas, once its own padding is off it.
 *
 * A node's radius is in pixels on the screen and a place is not, and sigma
 * draws a node the same size at every place, so two discs only keep off each
 * other while the places are counted in the pixels the discs are drawn at.
 * sigma fits the longer side of what it is given into the shorter side of its
 * canvas, less that padding, so this is the fewest pixels a place is ever
 * worth. A wider canvas only spreads the same graph further, which leaves
 * more room between two discs and never less.
 *
 * On a wider window the height binds instead: the shortest card that draws a
 * force graph is 448 pixels high (`h-[28rem]` on the Repositories tab,
 * against `h-[32rem]` on the Files tab) and its legend and gap take 24 of
 * those, which leaves 424 against the 354 of {@link VIEW_WIDTH_PX}. That is
 * the wider side of the two, so it is not what a graph is laid out for.
 */
const VIEW_SPAN_PX = VIEW_WIDTH_PX - 2 * STAGE_PADDING_PX
/** How many iterations push the discs of a crowded graph off each other. */
const ANTI_COLLISION_ITERATIONS = 60
/** How many times the graph is fitted to the view's span and pushed apart again, at most. */
const SEPARATE_ROUNDS = 15
/** How many passes push the discs apart between two fits, at most. */
const SEPARATE_PASSES = 25
/** How many times the discs are drawn smaller, where a push still leaves one covered. */
const SHRINK_TRIES = 4
/** How much of the size they had the discs are drawn at, on each further try. */
const SHRINK = 0.8
/** How much clear space is left between two discs, in pixels. */
const SEPARATE_GAP_PX = 2
/**
 * How much of the view's room the discs may take up and still be pushed off
 * each other. Above this they are drawn smaller instead: no arrangement of
 * discs covers all of what holds it, and pushing against one that is nearly
 * full only moves a disc out of one neighbour and into the next.
 */
const PACKED = 0.6
/** The smallest a disc is drawn, in pixels: a node is seen, however crowded its graph. */
const LEAST_RADIUS_PX = 2
/** How many cells of the grid one column is counted as, to number them all. */
const CELL_COLUMNS = 1 << 20
/** The angle between two seeds of the spiral, which spreads them evenly. */
const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5))
/** How far apart, in node radii, the spiral puts two neighbouring seeds. */
const SEED_SPACING = 3

/**
 * How hard one edge pulls its ends together, from how many edges it stands
 * for: `1 + log2(count)`. A pair of files with 2467 edges between them then
 * pulls about twelve times as hard as a pair with one, and not two thousand
 * times, which no repulsion answers and which leaves the graph swinging.
 */
function dampedWeight(weight: number | undefined): number {
  return 1 + Math.log2(Math.max(weight ?? 1, 1))
}

/**
 * A copy of the graph with every node placed by ForceAtlas2, settled, and
 * with the discs of the view kept off each other.
 *
 * It runs in three steps. ForceAtlas2 finds the shape: what an edge joins
 * ends near it, and what nothing joins ends outside. The graph is then fitted
 * to the span the view draws it in, because a radius is in pixels and a place
 * is not, and ForceAtlas2 runs again over the discs themselves
 * (`adjustSizes`), which pushes apart what covers what. {@link separate}
 * finishes that: ForceAtlas2 caps the force on a node, so a node buried under
 * its neighbours crawls out over hundreds of iterations and two nodes in one
 * place push each other nowhere at all.
 *
 * The graph comes back {@link VIEW_SPAN_PX} across, and what the pixels say
 * here is then what the view draws, or better: it fits what it is given into
 * its box, and every box a force graph is drawn in is at least that span.
 *
 * A graph whose discs cover more of that box than any arrangement of them
 * fits in — six hundred files of the size the Files tab gives them cover
 * several cards — is drawn with smaller discs rather than with one node over
 * another: {@link separate} takes the same share off every radius, so the
 * biggest node stays the biggest, and none goes under
 * {@link LEAST_RADIUS_PX}. The pointer reads such a graph by zooming in,
 * where the places grow faster than the discs do.
 *
 * Every node starts on the spiral, whatever place the model gave it: a
 * force layout is asked for where there is no place to keep, and a model
 * that puts its nodes in one place would otherwise hand back that place.
 */
export function forceLayout(graph: KnowledgeGraphModel): KnowledgeGraphModel {
  const copy = graph.copy()
  const seeds = spiral(copy)
  for (const [node, place] of seeds) copy.mergeNodeAttributes(node, place)
  if (copy.order < 2) return copy

  iterate(copy, ITERATIONS, { ...common(copy.order), scalingRatio: SCALING_RATIO })
  keepFinite(copy, seeds)
  scaleToWidth(copy, VIEW_SPAN_PX)
  iterate(copy, ANTI_COLLISION_ITERATIONS, {
    ...common(copy.order),
    adjustSizes: true,
    scalingRatio: 1,
  })
  keepFinite(copy, seeds)
  separate(copy)
  return copy
}

/**
 * The seed back, for a node ForceAtlas2 left without a number for a place.
 *
 * It divides by the distance between two nodes, so two nodes that land in
 * one place leave both with no place, and a run that starts on one of them
 * carries it to every node. A node is drawn somewhere, always.
 */
function keepFinite(graph: KnowledgeGraphModel, seeds: Map<string, { x: number; y: number }>) {
  graph.forEachNode((node, attributes) => {
    if (Number.isFinite(attributes.x) && Number.isFinite(attributes.y)) return
    graph.mergeNodeAttributes(node, seeds.get(node) ?? { x: 0, y: 0 })
  })
}

/**
 * The graph with no disc on top of another, at the span the view draws it
 * in, and with the discs drawn smaller where that is the only way to it.
 *
 * The pushes work in the pixels the discs are drawn at, since that is the
 * only place a radius means anything: a place is not in pixels, and the view
 * fits whatever it is given into its box, so a graph kept clear at the span
 * of that box is drawn clear.
 *
 * Some graphs hold more disc than box. The Files tab of a large repository
 * covers several cards with its discs, and no arrangement of discs fits them
 * in a box they cover: pushing there only moves a disc out of one neighbour
 * and into the next. {@link shrinkToFit} answers that first, by drawing every
 * disc smaller by the one share the area asks for. A push that still leaves a
 * pair covered — the area says what fits, not what an arrangement reaches —
 * takes another {@link SHRINK} off them.
 */
function separate(graph: KnowledgeGraphModel) {
  shrinkToFit(graph)
  for (let tries = 1; tries < SHRINK_TRIES; tries++) {
    if (cleared(graph, VIEW_SPAN_PX)) return
    shrink(graph, SHRINK)
  }
  cleared(graph, VIEW_SPAN_PX)
}

/**
 * Every disc drawn small enough that they all fit in the view, with the room
 * between them a push needs to work in.
 *
 * One share comes off every radius, so the node with the most symbols is
 * still the biggest node on the screen.
 */
function shrinkToFit(graph: KnowledgeGraphModel) {
  const room = PACKED * roomIn(VIEW_SPAN_PX)
  const discs = discArea(graph)
  // Area goes with the square of a radius, so this is the share of a radius.
  if (discs > room) shrink(graph, Math.sqrt(room / discs))
}

/** Every disc drawn `share` of the size it was, and none too small to see. */
function shrink(graph: KnowledgeGraphModel, share: number) {
  graph.forEachNode((node, attributes) => {
    const radius = attributes.size ?? NODE_SIZE
    graph.mergeNodeAttributes(node, { size: Math.max(LEAST_RADIUS_PX, radius * share) })
  })
}

/** How much room the discs of the graph take up, with their gaps. */
function discArea(graph: KnowledgeGraphModel): number {
  let area = 0
  graph.forEachNode((_node, attributes) => {
    const radius = (attributes.size ?? NODE_SIZE) + SEPARATE_GAP_PX / 2
    area += Math.PI * radius * radius
  })
  return area
}

/** How much room a graph of this span has: ForceAtlas2 leaves it round. */
function roomIn(span: number): number {
  return (Math.PI * span * span) / 4
}

/**
 * The rounds at one width, and whether a fit to it found nothing covered.
 *
 * A round fits the graph to the width and then pushes the discs apart until
 * none covers another. The fit comes first, because the pushes of the round
 * before it widened the graph, and the last thing done is a fit as well, so
 * the pixels that were measured are the pixels handed back.
 *
 * Where the discs do not fit in the width, the pushes move a disc out of one
 * neighbour and into the next and the fit takes back what they won, round
 * after round; only more room ends that, so the rounds are capped. The last
 * fit is looked at as well, since it is what the caller is handed: the
 * answer is what the pixels say after it, and never what they said before.
 */
function cleared(graph: KnowledgeGraphModel, width: number): boolean {
  for (let round = 0; round < SEPARATE_ROUNDS; round++) {
    scaleToWidth(graph, width)
    if (!spread(graph)) return true
  }
  scaleToWidth(graph, width)
  return !covers(graph)
}

/**
 * Passes over the covered pairs until none is left, and whether any pass
 * moved a disc.
 *
 * One pass only halves what a pair shares, and a disc pushed out of one
 * neighbour lands in the next, so the passes repeat. They do not fit the
 * graph in between: a fit shrinks what the passes just won, and a pass that
 * starts from a shrunken graph makes the same move again.
 */
function spread(graph: KnowledgeGraphModel): boolean {
  let moved = false
  for (let pass = 0; pass < SEPARATE_PASSES; pass++) {
    if (!pushApart(graph)) return moved
    moved = true
  }
  return moved
}

/**
 * One pass over the pairs of discs that cover each other, pushing each pair
 * apart by half of what it shares, and whether it moved anything.
 */
function pushApart(graph: KnowledgeGraphModel): boolean {
  const discs = discsOf(graph)
  let moved = false
  eachCoveredPair(discs, SEPARATE_GAP_PX, (one, other, alongX, alongY, distance, least) => {
    const push = (least - distance) / 2 / distance
    one.x -= alongX * push
    one.y -= alongY * push
    other.x += alongX * push
    other.y += alongY * push
    moved = true
  })
  if (!moved) return false

  for (const disc of discs) graph.mergeNodeAttributes(disc.node, { x: disc.x, y: disc.y })
  return true
}

/**
 * Whether any disc covers another, where the graph stands.
 *
 * It asks for nothing between them, unlike the pushes, which leave
 * {@link SEPARATE_GAP_PX} of room: a push only halves what a pair shares, so
 * a pair that is asked for that room lands just short of it, and a width
 * where every pair is just short of it is a width that is clear.
 */
function covers(graph: KnowledgeGraphModel): boolean {
  let found = false
  eachCoveredPair(discsOf(graph), 0, () => {
    found = true
  })
  return found
}

/** Every node as a disc, in node order: where it is, and how big it is drawn. */
function discsOf(graph: KnowledgeGraphModel): Disc[] {
  const discs: Disc[] = []
  graph.forEachNode((node, attributes) => {
    discs.push({
      node,
      order: discs.length,
      x: attributes.x ?? 0,
      y: attributes.y ?? 0,
      radius: attributes.size ?? NODE_SIZE,
    })
  })
  return discs
}

/**
 * Every pair of discs that covers each other, once, with the line between
 * them and how far apart the two should be.
 *
 * The pairs are found through a grid of cells as wide as the widest pair of
 * discs, so a disc is only measured against the discs in its own cell and
 * the eight around it, and a graph of a few thousand nodes costs its own
 * size rather than its square. A visitor that moves a disc is read by the
 * pairs after it, which is what lets one pass answer a whole neighbourhood.
 */
function eachCoveredPair(
  discs: Disc[],
  gap: number,
  visit: (
    one: Disc,
    other: Disc,
    alongX: number,
    alongY: number,
    distance: number,
    least: number,
  ) => void,
) {
  let widest = 0
  for (const disc of discs) widest = Math.max(widest, disc.radius)
  const side = 2 * widest + gap
  const cells = new Map<number, Disc[]>()
  for (const disc of discs) {
    const cell = cells.get(cellOf(disc.x, disc.y, side))
    if (cell) cell.push(disc)
    else cells.set(cellOf(disc.x, disc.y, side), [disc])
  }

  for (const one of discs) {
    const column = Math.floor(one.x / side)
    const row = Math.floor(one.y / side)
    for (let across = -1; across <= 1; across++) {
      for (let down = -1; down <= 1; down++) {
        for (const other of cells.get((column + across) * CELL_COLUMNS + row + down) ?? []) {
          // Every pair once, and no disc against itself.
          if (other.order <= one.order) continue
          const least = one.radius + other.radius + gap
          let alongX = other.x - one.x
          let alongY = other.y - one.y
          let distance = Math.hypot(alongX, alongY)
          if (distance >= least) continue
          if (distance === 0) {
            // Two discs in one place have no direction to part along. This
            // gives them one, and the same one on every run.
            alongX = Math.cos(one.order * GOLDEN_ANGLE)
            alongY = Math.sin(one.order * GOLDEN_ANGLE)
            distance = 1
          }
          visit(one, other, alongX, alongY, distance, least)
        }
      }
    }
  }
}

/** The cell a place falls in, as one number: the grid is read, never read back. */
function cellOf(x: number, y: number, side: number): number {
  return Math.floor(x / side) * CELL_COLUMNS + Math.floor(y / side)
}

/** One node while the discs are pushed apart: where it is, and how big it is drawn. */
interface Disc {
  node: string
  /** Its place in node order, which decides what is measured against what. */
  order: number
  x: number
  y: number
  radius: number
}

/** One run of ForceAtlas2 over the graph, with the damped weight of each edge. */
function iterate(graph: KnowledgeGraphModel, iterations: number, settings: ForceAtlas2Settings) {
  forceAtlas2.assign<GraphNodeAttributes, GraphEdgeAttributes>(graph, {
    iterations,
    getEdgeWeight: (_edge, attributes) => dampedWeight(attributes.weight),
    settings,
  })
}

/** What every run is run with, for a graph of this many nodes. */
function common(order: number): ForceAtlas2Settings {
  return {
    linLogMode: false,
    outboundAttractionDistribution: false,
    // The weight is damped already: one is the pull of one edge.
    edgeWeightInfluence: 1,
    strongGravityMode: false,
    gravity: 1,
    slowDown: SLOW_DOWN,
    barnesHutOptimize: order >= BARNES_HUT_NODES,
    barnesHutTheta: 0.5,
  }
}

/** The same graph, about `width` across and around the origin. */
function scaleToWidth(graph: KnowledgeGraphModel, width: number) {
  let minX = Number.POSITIVE_INFINITY
  let maxX = Number.NEGATIVE_INFINITY
  let minY = Number.POSITIVE_INFINITY
  let maxY = Number.NEGATIVE_INFINITY
  graph.forEachNode((_node, attributes) => {
    const x = attributes.x ?? 0
    const y = attributes.y ?? 0
    minX = Math.min(minX, x)
    maxX = Math.max(maxX, x)
    minY = Math.min(minY, y)
    maxY = Math.max(maxY, y)
  })
  const across = Math.max(maxX - minX, maxY - minY)
  if (!Number.isFinite(across) || across <= 0) return
  const scale = width / across
  const centreX = (minX + maxX) / 2
  const centreY = (minY + maxY) / 2
  graph.forEachNode((node, attributes) => {
    graph.mergeNodeAttributes(node, {
      x: ((attributes.x ?? 0) - centreX) * scale,
      y: ((attributes.y ?? 0) - centreY) * scale,
    })
  })
}

/**
 * A place for every node: a spiral, in node order, whose turns are as far
 * apart as the widest node is.
 *
 * It is what makes the run deterministic and finite. The order of the nodes
 * decides the seed, and every seed is a different point: a ring of hundreds
 * of nodes puts them on top of each other, and two nodes in one place are a
 * division by zero.
 *
 * A place the model gave a node is dropped, since a force layout is asked
 * for where the model has no places of its own. `placed` is what keeps them
 * (`graph-model.ts`), for the layouts that do.
 */
function spiral(graph: KnowledgeGraphModel): Map<string, { x: number; y: number }> {
  let widest = 0
  graph.forEachNode((_node, attributes) => {
    widest = Math.max(widest, attributes.size ?? NODE_SIZE)
  })
  const spacing = SEED_SPACING * widest
  const seeds = new Map<string, { x: number; y: number }>()
  let step = 0
  graph.forEachNode((node) => {
    // The half keeps the first node off the centre, where gravity divides by
    // the distance to it.
    const radius = spacing * Math.sqrt(step + 0.5)
    const angle = step * GOLDEN_ANGLE
    seeds.set(node, { x: radius * Math.cos(angle), y: radius * Math.sin(angle) })
    step++
  })
  return seeds
}
