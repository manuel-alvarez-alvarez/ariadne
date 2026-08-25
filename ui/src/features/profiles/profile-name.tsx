/**
 * A profile id, shown as the profile, and as the way to it.
 *
 * Goals, tasks, reviews and sessions all carry profile *ids* — 26-character
 * ULIDs, which two reviewers of the same task are indistinguishable as. The
 * name is what a person recognises, so it is what every one of those surfaces
 * shows, and it links to the profile itself ({@link paths.profile}, which
 * expands its row on the profiles screen): a name is accepted anywhere an id
 * is, so there was nothing to copy the ULID for, and this was the one entity
 * mention in the app that went nowhere.
 *
 * The names come from the profiles list (`qk.profiles.list()`), the same key
 * the profiles screen and the sessions table read: one request serves every id
 * on screen, and whichever screen loads it first serves the others. A profile
 * that is not in the list — deleted since, or not loaded yet — falls back to
 * its id in the plain mono face, still inside the link: the click belongs to
 * the link either way, and the screen simply finds no row to expand.
 */

import { useQuery } from "@tanstack/react-query"
import { Link } from "react-router-dom"

import { cn } from "@/lib/format"
import { paths } from "@/routes/paths"
import { profilesQueryOptions } from "./queries"

export function ProfileName({ profileId, className }: { profileId: string; className?: string }) {
  const profiles = useQuery(profilesQueryOptions())
  const name = profiles.data?.find((profile) => profile.id === profileId)?.name
  return (
    <Link
      to={paths.profile(profileId)}
      // The id keeps the hover, as it did on the copy button: it is what the
      // terminal wants, and reading it off the mention beats opening the page.
      title={profileId}
      className={cn(
        "inline-block max-w-full truncate underline-offset-3 hover:underline",
        !name && "font-mono",
        className,
      )}
    >
      {name ?? profileId}
    </Link>
  )
}
