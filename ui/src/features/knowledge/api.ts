/**
 * The four knowledge-base routes (022), typed the same way every other
 * endpoint is — an `openapi-fetch` client over `paths` — but against a `paths`
 * shape written by hand rather than generated: task 1 lands the daemon side of
 * 022 separately, so `schema.d.ts` does not carry these routes yet, and it is
 * "DO NOT EDIT BY HAND" for good reason.
 *
 * `api()` already resolves to the client the settings store points at the
 * daemon's own base URL, and that is a runtime concern only — the generic
 * parameter below is compile-time — so casting its return value onto this
 * local `paths` gets everything `unwrap` and the rest of the app expect for
 * free. Delete this file and read straight off `@/api`'s own `api()` once
 * `npm run gen:api` picks these routes up for real.
 */

import type { Client } from "openapi-fetch"

import { api } from "@/api"

import type {
  KnowledgeInteractionGroupDto,
  KnowledgeSearchResultDto,
  KnowledgeStatusDto,
} from "./types"

interface KnowledgePaths {
  "/v1/repositories/{repository_id}/knowledge": {
    get: {
      parameters: {
        path: { repository_id: string }
      }
      responses: {
        200: { content: { "application/json": KnowledgeStatusDto } }
        404: { content?: never }
      }
    }
  }
  "/v1/repositories/{repository_id}/knowledge/reindex": {
    post: {
      parameters: {
        path: { repository_id: string }
      }
      responses: {
        202: { content?: never }
        404: { content?: never }
      }
    }
  }
  "/v1/knowledge/search": {
    get: {
      parameters: {
        query?: {
          q?: string
          repository?: string
          git_ref?: string
          kind?: string
          path?: string
          limit?: number
        }
      }
      responses: {
        200: { content: { "application/json": KnowledgeSearchResultDto[] } }
      }
    }
  }
  "/v1/knowledge/interactions": {
    get: {
      parameters: {
        query?: {
          repository?: string
          git_ref?: string
        }
      }
      responses: {
        200: { content: { "application/json": KnowledgeInteractionGroupDto[] } }
      }
    }
  }
}

export function knowledgeApi(): Client<KnowledgePaths> {
  return api() as unknown as Client<KnowledgePaths>
}
