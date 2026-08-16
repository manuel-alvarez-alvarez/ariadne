/**
 * Every URL in the app, in one place. Link with these helpers instead of
 * hand-writing paths so a feature can move its routes without breaking the
 * links other features have to it.
 */

export const paths = {
  goals: () => "/goals",
  goal: (goalId: string) => `/goals/${goalId}`,
  task: (taskId: string) => `/tasks/${taskId}`,
  sessions: () => "/sessions",
  session: (sessionId: string) => `/sessions/${sessionId}`,
  profiles: () => "/profiles",
} as const
