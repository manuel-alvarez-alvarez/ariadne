/**
 * The content blocks of a message or of a call that are not text. An image
 * shows inline. An audio block, a resource and a resource link show as a
 * labelled row with a link. A block with bytes carries a `blob` path of the
 * daemon (040), which the host's address makes whole.
 */

import { FileAudioIcon, FileIcon, LinkIcon } from "lucide-react"

import { type ContentBlockDto, type Host, hostBaseUrl } from "@/api"

/** The whole address of a block's bytes on the daemon of `host`. */
function blobUrl(host: Host, blob: string): string {
  return `${hostBaseUrl(host) ?? ""}${blob}`
}

/** Each block that is not text, in its order. Text blocks draw nothing here. */
export function MediaBlocks({ host, blocks }: { host: Host; blocks: readonly ContentBlockDto[] }) {
  const media = blocks.filter((block) => block.type !== "text")
  if (media.length === 0) return null
  return (
    <div className="mt-1 space-y-1">
      {media.map((block, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: the blocks of one content, never reordered
        <MediaBlock key={index} host={host} block={block} />
      ))}
    </div>
  )
}

function MediaBlock({ host, block }: { host: Host; block: ContentBlockDto }) {
  switch (block.type) {
    case "text":
      return null
    case "image": {
      const src = block.blob ? blobUrl(host, block.blob) : block.uri
      if (!src) return <LinkRow icon="file" label="Image" detail={block.mime_type} />
      return (
        <img
          src={src}
          alt={block.uri ? `Image ${block.uri}` : "Image"}
          className="max-h-80 max-w-full rounded-md border border-border/60"
        />
      )
    }
    case "audio":
      return (
        <LinkRow
          icon="audio"
          label="Audio"
          detail={block.mime_type}
          href={block.blob ? blobUrl(host, block.blob) : undefined}
        />
      )
    case "resource":
      return (
        <LinkRow
          icon="file"
          label="Resource"
          detail={block.uri}
          href={block.blob ? blobUrl(host, block.blob) : block.uri}
        />
      )
    case "resource_link":
      return (
        <LinkRow
          icon="link"
          label="Resource link"
          detail={block.title ?? block.name}
          href={block.uri}
        />
      )
  }
}

const ICONS = { audio: FileAudioIcon, file: FileIcon, link: LinkIcon }

function LinkRow({
  icon,
  label,
  detail,
  href,
}: {
  icon: keyof typeof ICONS
  label: string
  detail?: string | null
  href?: string
}) {
  const Icon = ICONS[icon]
  return (
    <div
      data-content-row={label}
      className="flex min-w-0 items-center gap-2 rounded-md border border-border/60 px-2 py-1 font-mono text-xs"
    >
      <Icon aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="shrink-0 text-muted-foreground">{label}</span>
      {href ? (
        <a href={href} className="min-w-0 truncate text-foreground underline underline-offset-2">
          {detail || href}
        </a>
      ) : (
        <span className="min-w-0 truncate">{detail}</span>
      )}
    </div>
  )
}
