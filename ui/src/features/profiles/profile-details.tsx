/**
 * Everything a profile holds — the UI's `profile inspect`, plus the writes the
 * table has no room for.
 *
 * The prompts are the reason this is a panel rather than more columns: they are
 * the long, whitespace-significant fields, and the table has nowhere to put
 * them. They are shown here and edited nowhere but the profile dialog (see
 * {@link ProfilePrompts}).
 */

import { useQuery } from "@tanstack/react-query"

import type { ProfileDto } from "@/api"
import { Fact, FactList } from "@/components/fact-list"
import { When } from "@/components/when"

import { modelRefLabel } from "./model-ref"
import { roleLabel } from "./profile-labels"
import { ProfilePrompts } from "./profile-prompts"
import { modelsQueryOptions } from "./queries"

/** What the daemon resolves for a profile that pins nothing. */
const UNPINNED_HINT = "first installed CLI, at spawn time, on its own default model"

export function ProfileDetails({ profile }: { profile: ProfileDto }) {
  // What the pinned model can do, when the catalog knows it. The id carries
  // the agent CLI, so it is the whole key; a model the catalog does not list —
  // free text, or the catalog failing to load — shows exactly as before, so
  // nothing here waits on the request.
  const models = useQuery(modelsQueryOptions())
  const catalogModel = profile.model
    ? models.data?.find((model) => model.id === profile.model)
    : undefined
  // No pin is a fact about the profile rather than a missing value, so it
  // reads as the word `auto` with what the daemon does instead beside it.
  const unpinned = !profile.model

  return (
    <div className="flex flex-col gap-5 py-2">
      {/* Unframed: this block already sits inside the expanded row's own box,
          where every other panel's facts sit on the screen behind them. */}
      <FactList framed={false}>
        <Fact label="Role">{roleLabel(profile.role)}</Fact>
        {/* One fact, since one string is the whole choice: the agent CLI and,
            after a `:`, the model of it. What the daemon does instead of a pin
            is in the hint, which is the only place a truncated row says
            anything in full; the catalog's blurb is the opposite — prose rather
            than an identifier — so it takes a wrapping line of its own. */}
        <Fact
          label="Model"
          hint={unpinned ? UNPINNED_HINT : undefined}
          caption={catalogModel?.description}
        >
          {/* Block, both of them: `truncate` is `overflow` plus `text-overflow`,
              and neither applies to an inline box — an inline span wearing it
              paints its whole line straight out of the column instead of
              ending in an ellipsis. The word and the long hint after it are
              one line together, so the cut has to fall wherever the column
              ends and the hint above carries the rest. */}
          {unpinned ? (
            <span className="block truncate text-muted-foreground italic">
              {modelRefLabel(profile.model)}
              <span className="ml-1.5 text-xs">({UNPINNED_HINT})</span>
            </span>
          ) : (
            <span className="block truncate font-mono">{modelRefLabel(profile.model)}</span>
          )}
        </Fact>
        <Fact label="Created">
          <When at={profile.created_at} label="created" />
        </Fact>
        <Fact label="Updated">
          <When at={profile.updated_at} label="updated" />
        </Fact>
      </FactList>

      <ProfilePrompts profile={profile} />
    </div>
  )
}
