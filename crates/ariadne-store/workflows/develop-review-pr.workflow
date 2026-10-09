workflow develop-review-pr
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
  pr[Pull request]
    Push the branch, open the request and keep it until a human merges or closes it.
    skills: pr-babysit
    rank: balanced
    gate: request-merged
