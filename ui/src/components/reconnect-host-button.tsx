/**
 * The reconnect button of a named host, in the header picker and in the
 * settings Hosts section: `POST /v1/hosts/{name}/reconnect` of the settings
 * daemon. The answer is the host's row, and `host_updated` is what moves it
 * on screen. `local` has no button.
 */

import { useMutation } from "@tanstack/react-query"
import { RefreshCwIcon } from "lucide-react"
import type { MouseEvent } from "react"
import { toast } from "sonner"

import { api, LOCAL_HOST, unwrap } from "@/api"
import { Button } from "@/components/ui/button"
import { describeError } from "@/lib/format"

export function ReconnectHostButton({ name }: { name: string }) {
  const reconnect = useMutation({
    mutationFn: () =>
      unwrap(api(LOCAL_HOST).POST("/v1/hosts/{name}/reconnect", { params: { path: { name } } })),
    onError: (error) =>
      toast.error(`Could not reconnect ${name}`, { description: describeError(error) }),
  })
  if (name === LOCAL_HOST) return null

  // Inside a picker row the click is this button's alone: it picks nothing.
  const stop = (event: MouseEvent) => event.stopPropagation()
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon-xs"
      aria-label={`Reconnect ${name}`}
      disabled={reconnect.isPending}
      onPointerUp={stop}
      onClick={(event) => {
        stop(event)
        reconnect.mutate()
      }}
    >
      <RefreshCwIcon />
    </Button>
  )
}
