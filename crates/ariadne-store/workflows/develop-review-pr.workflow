workflow develop-review-pr
  develop[Develop]
    Build the task on its branch and commit it.
    skills: coding
    ranks: balanced
    gate: committed
  review[Review]
    Judge the change against the task and the repository rules.
    skills: code-review
    ranks: balanced, frontier
  pr[Pull request]
    Keep the task's request until it merges or closes.
    skills: pr-babysit
    ranks: balanced
    gate: request-merged
