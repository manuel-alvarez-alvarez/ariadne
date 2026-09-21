/**
 * The knowledge graph's renderer, as the test runner sees it.
 *
 * sigma.js draws on WebGL, which jsdom does not have — sigma reads
 * `WebGLRenderingContext` as its module loads — so each test file that draws a
 * graph swaps `@/features/knowledge/graph/sigma-canvas` for this with `vi.mock`. It draws what sigma
 * would be told: a button per node and per edge, each carrying what the
 * reducers said about it, and it fires the same callbacks sigma's pointer
 * events would. The model and the reducers are the app's own; only the
 * pixels are left out. A node or an edge the reducers hide is `hidden`, so a
 * role query does not find it, and `data-x` and `data-y` are where the
 * layout put a node before the first frame.
 */

type SigmaCanvasProps = Parameters<
  typeof import("@/features/knowledge/graph/sigma-canvas").SigmaCanvas
>[0]

export function SigmaCanvas({
  graph,
  layout,
  nodeReducer,
  edgeReducer,
  onEnterNode,
  onLeaveNode,
  onClickNode,
  onClickEdge,
}: SigmaCanvasProps) {
  return (
    <div data-testid="graph-canvas" data-layout={layout}>
      <ul aria-label="Graph nodes">
        {graph.mapNodes((node, attributes) => {
          const drawn = nodeReducer(node, attributes)
          return (
            <li key={node} hidden={drawn.hidden}>
              <button
                type="button"
                data-node={node}
                data-color={drawn.color}
                data-label={drawn.label}
                data-highlighted={drawn.highlighted}
                data-force-label={drawn.forceLabel}
                data-x={attributes.x}
                data-y={attributes.y}
                onMouseEnter={() => onEnterNode(node)}
                onMouseLeave={onLeaveNode}
                onClick={() => onClickNode(node)}
              >
                {attributes.label}
              </button>
            </li>
          )
        })}
      </ul>
      <ul aria-label="Graph edges">
        {graph.mapEdges((edge, attributes, _source, _target, from, to) => {
          const drawn = edgeReducer(edge, attributes)
          return (
            <li key={edge} hidden={drawn.hidden}>
              <button
                type="button"
                data-edge={edge}
                data-color={drawn.color}
                data-label={drawn.label}
                data-force-label={drawn.forceLabel}
                data-type={drawn.type}
                onClick={() => onClickEdge(edge)}
              >
                {from.label} → {to.label}
              </button>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
