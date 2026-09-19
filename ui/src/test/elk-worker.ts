/**
 * The ELK layout worker, as the test runner sees it.
 *
 * jsdom has no `Worker`, so each test file that lays a graph out swaps
 * `elkjs/lib/elk-worker.min.js?worker` for this with `vi.mock`. It is
 * elkjs's own stand-in: the real layout engine, run on the calling thread
 * behind the same message interface.
 */

import * as elk from "elkjs/lib/elk-worker.js"

// The package types its stand-in as a type only, though it exports the class.
export default (elk as unknown as { Worker: new () => Worker }).Worker
