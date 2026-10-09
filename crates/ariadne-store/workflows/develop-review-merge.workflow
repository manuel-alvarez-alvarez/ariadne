workflow develop-review-merge
  develop[Develop]
    Build the task on its branch and commit it.
    skills: coding
    rank: balanced
    gate: committed
  review[Review]
    Run the whole suite and judge the change against the task and the repository rules.
    Fail the step with the changes to make.
    skills: code-review
    rank: frontier
  merge[Merge]
    Rebase onto the base branch, run the whole suite, squash, fast-forward and push.
    skills: merge
    rank: fast
    gate: merged
