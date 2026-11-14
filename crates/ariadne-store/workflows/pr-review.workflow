workflow pr-review
  review[Review]
    Review the request and each later push until it merges or closes.
    skills: pr-reviewer
    ranks: balanced, frontier
    gate: request-merged
