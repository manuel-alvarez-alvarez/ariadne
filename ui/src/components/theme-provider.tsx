/**
 * Light/dark theming. `next-themes` works fine outside Next.js: it toggles the
 * `.dark` class the Tailwind theme in `index.css` keys off, and persists the
 * choice itself.
 */

import { ThemeProvider as NextThemeProvider } from "next-themes"
import type { ReactNode } from "react"

const THEME_STORAGE_KEY = "ariadne.theme"

export function ThemeProvider({ children }: { children: ReactNode }) {
  return (
    <NextThemeProvider
      attribute="class"
      defaultTheme="system"
      enableSystem
      disableTransitionOnChange
      storageKey={THEME_STORAGE_KEY}
    >
      {children}
    </NextThemeProvider>
  )
}
