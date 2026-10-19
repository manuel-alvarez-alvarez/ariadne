# Dashboards

See what the work did, as a dashboard: panels of charts, tables and text
over the daemon's own metrics and logs, edited in place rather than read
only. Ariadne ships one, `stats`, and keeps any others you save over it or
add.

## What the screen shows

Open **Stats** in the sidebar. The picker at the top names every dashboard
the daemon knows; pick one to open it below. What opens is a real Perses
dashboard — the same open-source tool, embedded — so every panel it draws
reads straight off the daemon: a chart's query runs against the daemon's own
Prometheus-compatible metrics API, and a log panel's against its
Loki-compatible one. Nothing here aggregates on its own; a panel is only ever
as fresh as the query behind it.

## Creating, copying, renaming and deleting

**New dashboard**, beside the picker, asks for a name and what it starts
from: **Empty**, a blank dashboard with nothing on it, or `Copy of
<picked dashboard>`, which takes the picked dashboard's own panels, layouts,
variables and time settings and files them under the new name — a starting
point rather than a blank one.

**Rename** files the picked dashboard's document under the new name and
then removes the old one, so a name already taken, or any other refusal,
leaves the old dashboard exactly as it was. **Delete** asks for confirmation
first, since there is no undo. Neither is offered for the built-in `stats`
dashboard: reset it instead.

Whichever of the three you use, the picker opens on the dashboard it names —
the new one after a create or a rename, and a remaining one after a delete.

## Editing a panel

The dashboard opens read-only. Click **Edit**, in the dashboard's own
toolbar, to add a panel, resize or move one, change a query, or edit the
text of a Markdown panel, the same way you would in Perses itself. **Save**
writes the change and returns to the read-only view; **Cancel** discards it
and returns there too.

## Saving

Save from the dashboard's own toolbar. It writes the whole document back to
the daemon, so every panel, every variable and the layout itself are kept
together, not just the one thing you touched. A save made from another
window, or from the CLI, shows up here too: the screen notices and reopens
the dashboard on what was just saved, rather than leaving you looking at what
you had open before.

## Resetting

**Reset**, beside the picker, throws away everything saved over a shipped
dashboard and puts it back on the one Ariadne ships. It is disabled once a
dashboard is already on that shipped document — there is nothing to go back
to — and it asks before it acts, since there is no undo. A dashboard that is
entirely your own has no shipped document behind it, so there is nothing to
reset it to.

## From the CLI

`ariadne dashboard` lists and shows the daemon's dashboards from the
terminal, the way `ariadne skill` and `ariadne workflow` do for theirs: a
shipped one is reset rather than deleted, and one of your own is deleted
rather than reset. See [Using the CLI](cli.md) for the table flags and
`--format json` every command shares.
