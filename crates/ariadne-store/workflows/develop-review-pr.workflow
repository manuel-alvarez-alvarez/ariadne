workflow develop-review-pr
  develop[Develop]
    Build the task on its branch and commit it.
    skills: coding
    ranks: balanced
    gate: committed
  review[Review]
    Judge the change against the task and the repository rules.
    Fail the step with the changes to make.
    skills: code-review
    ranks: balanced, frontier
  pr[Pull request]
    Push the branch, open the request and keep it until it merges or closes.
    skills: pr-babysit
    ranks: balanced
    gate: request-merged
