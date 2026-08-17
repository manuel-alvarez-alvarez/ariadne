/**
 * A profile id, shown as the profile.
 *
 * Goals, tasks, reviews and sessions all carry profile *ids* — 26-character
 * ULIDs, which two reviewers of the same task are indistinguishable as. The
 * name is what a person recognises, so it is what every one of those surfaces
 * shows, with the id still one click away on the clipboard for the terminal.
 *
 * The names come from the profiles list (`qk.profiles.list()`), the same key
 * the profiles screen and the sessions table read: one request serves every id
 * on screen, and whichever screen loads it first serves the others. A profile
 * that is not in the list — deleted since — falls back to its id, which is all
 * that is left of it.
 */

import { useQuery } from "@tanstack/react-query"

import { CopyableId } from "@/components/copyable-id"
import { profilesQueryOptions } from "./queries"

export function ProfileName({ profileId, className }: { profileId: string; className?: string }) {
  const profiles = useQuery(profilesQueryOptions())
  const name = profiles.data?.find((profile) => profile.id === profileId)?.name
  return (
    <CopyableId
      value={profileId}
      // The id stays the copied value and the hover title; only what is drawn
      // is the name — as a name, until there is none and the ULID is back.
      display={name ? () => name : undefined}
      face={name ? "name" : "mono"}
      label="profile id"
      className={className}
    />
  )
}
