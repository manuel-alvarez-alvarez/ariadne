workflow develop-review-merge
  develop[Develop]
    Build the task on its branch and commit it.
    skills: coding
    ranks: fast, balanced, frontier
    gate: committed
  review[Review]
    Judge the change against the task and the repository rules.
    skills: code-review
    ranks: balanced, frontier
  merge[Merge]
    Land the task on the base branch.
    skills: merge
    ranks: fast, balanced
    gate: merged
