/** What the host hands this page on its own query string (rule 2). */
interface PageParams {
  daemonUrl: string
  dashboardName: string
  theme: "light" | "dark"
}

export function readPageParams(search: string): PageParams {
  const params = new URLSearchParams(search)
  return {
    daemonUrl: params.get("daemon") ?? "",
    dashboardName: params.get("dashboard") ?? "",
    theme: params.get("theme") === "dark" ? "dark" : "light",
  }
}
