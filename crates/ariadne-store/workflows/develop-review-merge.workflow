workflow develop-review-merge
  develop[Develop]
    Build the task on its branch and commit it.
    skills: coding
    rank: balanced
    gate: committed
  review[Review]
    Judge the change against the task and the repository rules.
    Fail the step with the changes to make.
    skills: code-review
    rank: frontier
  merge[Merge]
    Rebase onto the base branch, squash, fast-forward and push.
    skills: merge
    rank: fast
    gate: merged
