/**
 * Persisted app settings. Currently just the daemon base URL — the address of
 * `tcp_listen` from `~/.ariadne/config.toml`.
 *
 * The store owns the API client's base URL: nothing else should call
 * `setApiBaseUrl`.
 */

import { create } from "zustand"
import { persist } from "zustand/middleware"

import { DEFAULT_BASE_URL, normalizeBaseUrl, setApiBaseUrl } from "@/api"

export const SETTINGS_STORAGE_KEY = "ariadne.settings"

interface SettingsState {
  /** e.g. `http://127.0.0.1:7676`, without a trailing slash. */
  baseUrl: string
  setBaseUrl: (url: string) => void
  resetBaseUrl: () => void
}

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      baseUrl: DEFAULT_BASE_URL,
      setBaseUrl: (url) => set({ baseUrl: normalizeBaseUrl(url) }),
      resetBaseUrl: () => set({ baseUrl: DEFAULT_BASE_URL }),
    }),
    {
      name: SETTINGS_STORAGE_KEY,
      partialize: (state) => ({ baseUrl: state.baseUrl }),
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
