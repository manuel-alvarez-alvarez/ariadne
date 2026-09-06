/**
 * A skill, shown as its name and as the way to it.
 *
 * An agent has no name of its own — it is a CLI, a model and a set of skills —
 * so a skill name is what every mention of an agent is actually made of, and
 * the one thing in such a mention a reader can follow. It links to the skill
 * itself ({@link paths.skill}, which opens it on the skills screen).
 *
 * The summaries come from the skills list (`qk.skills.list()`), the same key
 * the skills screen reads: one request serves every name on screen, and
 * whichever screen loads it first serves the others. A name no skill answers
 * to — deleted since, or not loaded yet — still links, and the screen it lands
 * on simply says it has no such skill.
 */

import { useQuery } from "@tanstack/react-query"
import { Link } from "react-router-dom"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"
import { paths } from "@/routes/paths"
import { skillsQueryOptions } from "./queries"

export function SkillName({ name, className }: { name: string; className?: string }) {
  const skills = useQuery(skillsQueryOptions())
  const summary = skills.data?.find((skill) => skill.name === name)?.summary

  return (
    // The name leads and the line it says about itself follows: a skill name
    // is short enough to survive a table cell, and what the reader wants from
    // hovering it is what the skill is for.
    //
    // The link stays inline, and clips nothing: an inline-block that hides its
    // overflow takes its *bottom edge* as its baseline, which lifts the name
    // off the line the rest of the mention sits on — the comma beside it and
    // the model after it. Whatever box holds the name is what truncates it.
    <Tooltip>
      <TooltipTrigger
        render={
          <Link
            to={paths.skill(name)}
            className={cn("underline-offset-3 hover:underline", className)}
          />
        }
      >
        {name}
      </TooltipTrigger>
      <TooltipContent>{summary ?? name}</TooltipContent>
    </Tooltip>
  )
}
