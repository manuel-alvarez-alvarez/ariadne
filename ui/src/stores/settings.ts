/**
 * Persisted app settings: the daemon base URL — the address of `tcp_listen`
 * from `~/.ariadne/config.toml` — and the bits of screen state that should
 * outlive both a navigation and a restart.
 *
 * The store owns the API client's base URL: nothing else should call
 * `setApiBaseUrl`.
 */

import { create } from "zustand"
import { persist } from "zustand/middleware"

import { DEFAULT_BASE_URL, normalizeBaseUrl, setApiBaseUrl } from "@/api"
import { DEFAULT_GOAL_STATUS_FILTER } from "@/features/goals/filters"

const SETTINGS_STORAGE_KEY = "ariadne.settings"

interface SettingsState {
  /** e.g. `http://127.0.0.1:7676`, without a trailing slash. */
  baseUrl: string
  setBaseUrl: (url: string) => void
  resetBaseUrl: () => void
  /**
   * The status filter the goals board was last left with, spelled the way its
   * `?status=` param spells it: `"active,completed"`, or `""` for all statuses.
   *
   * Kept as the param rather than as a parsed selection so the store stays out
   * of the goals feature: `readStatusFilter` is what makes sense of the value,
   * and one that has aged out of the daemon's statuses is dropped there like
   * any other bad `?status=`. The one thing borrowed from that feature is what
   * a board with nothing remembered opens on, which is the filter's own
   * default and not this store's to invent.
   */
  goalStatusFilter: string
  setGoalStatusFilter: (value: string) => void
  /**
   * The filters the sessions screen was last left with, each spelled the
   * way its own param spells it: `"failed"`, `"live"` or `"attention"` for the
   * status, `"engineer"` for the role, and `""` for no filter at all.
   *
   * Raw params again, for the same reason `goalStatusFilter` is one: the store
   * has no business knowing what a session status or a role is, and a value
   * that has aged out of the daemon's vocabulary is dropped where it is read.
   */
  sessionStatusFilter: string
  setSessionStatusFilter: (value: string) => void
  sessionRoleFilter: string
  setSessionRoleFilter: (value: string) => void
  /**
   * And the goal or task the screen was narrowed to, as the id its param
   * carries — the chip above the table. Remembered like the other two, and
   * cleared the same way: the screen shows what it was left showing, and the
   * chip is what says so.
   */
  sessionGoalFilter: string
  setSessionGoalFilter: (value: string) => void
  sessionTaskFilter: string
  setSessionTaskFilter: (value: string) => void
  /**
   * Whether the sidebar is folded down to an icon rail.
   *
   * Persisted rather than kept per window: it is how this user works, and a
   * board that fits a 1280px laptop with the rail down should still fit it
   * after a restart. See `components/app-shell.tsx` and the `[` chord.
   */
  sidebarCollapsed: boolean
  toggleSidebar: () => void
}

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      baseUrl: DEFAULT_BASE_URL,
      setBaseUrl: (url) => set({ baseUrl: normalizeBaseUrl(url) }),
      resetBaseUrl: () => set({ baseUrl: DEFAULT_BASE_URL }),
      goalStatusFilter: DEFAULT_GOAL_STATUS_FILTER,
      setGoalStatusFilter: (value) => set({ goalStatusFilter: value }),
      sessionStatusFilter: "",
      setSessionStatusFilter: (value) => set({ sessionStatusFilter: value }),
      sessionRoleFilter: "",
      setSessionRoleFilter: (value) => set({ sessionRoleFilter: value }),
      sessionGoalFilter: "",
      setSessionGoalFilter: (value) => set({ sessionGoalFilter: value }),
      sessionTaskFilter: "",
      setSessionTaskFilter: (value) => set({ sessionTaskFilter: value }),
      sidebarCollapsed: false,
      toggleSidebar: () => set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed })),
    }),
    {
      name: SETTINGS_STORAGE_KEY,
      partialize: (state) => ({
        baseUrl: state.baseUrl,
        goalStatusFilter: state.goalStatusFilter,
        sessionStatusFilter: state.sessionStatusFilter,
        sessionRoleFilter: state.sessionRoleFilter,
        sessionGoalFilter: state.sessionGoalFilter,
        sessionTaskFilter: state.sessionTaskFilter,
        sidebarCollapsed: state.sidebarCollapsed,
      }),
      onRehydrateStorage: () => (state) => {
        if (state) setApiBaseUrl(state.baseUrl)
      },
    },
  ),
)

// Keep the HTTP client pointed at the configured daemon, now and on every change.
setApiBaseUrl(useSettingsStore.getState().baseUrl)
useSettingsStore.subscribe((state, previous) => {
  if (state.baseUrl !== previous.baseUrl) setApiBaseUrl(state.baseUrl)
})

export const useBaseUrl = () => useSettingsStore((state) => state.baseUrl)
