# Ariadne Token Usage Analysis Summary

## Quick Facts

| Metric | Value |
|--------|-------|
| Total Tasks | 215 |
| Total Input Tokens | ~12.86 billion |
| Total Cached Input Tokens | ~12.71 billion |
| Total Output Tokens | ~33.31 million |
| **Total Tokens** | ~25.57 billion |
| **Estimated Cost** | ~$12,833 USD |
| **Cache Hit Rate** | 98.81% |

## Top 10 Cost Drivers (Tasks)

| Rank | Task | Cost | % of Total |
|------|------|------|------------|
| 1 | Show recovery blockers only when the user must act | $829.68 | 6.46% |
| 2 | Show pull requests only when the user has a next action | $562.29 | 4.38% |
| 3 | Babysit a request of mine opened by hand | $470.10 | 3.66% |
| 4 | Validate what reaches git, filesystem, registry | $445.52 | 3.47% |
| 5 | Start every pull request review manually | $413.59 | 3.22% |
| 6 | Make PR actions follow supervision | $405.52 | 3.16% |
| 7 | Cut daemon integration suite time | $349.80 | 2.73% |
| 8 | Land custom workflows branch | $319.48 | 2.49% |
| 9 | Fence forge text in prompts | $286.74 | 2.24% |
| 10 | Let agents edit forge posts | $268.21 | 2.09% |

**Top 10 Total: ~$4,331 USD (33.7% of total)**

## Top 5 Goals by Token Usage

| Rank | Goal | Total Tokens | Avg per Task |
|------|------|-------------|--------------|
| 1 | Show recovery blockers | 2.83B | 942M |
| 2 | Audit browser/local clients | 2.68B | 335M |
| 3 | Add supervise_posts knob | 2.53B | 230M |
| 4 | Add workflow catalog | 2.08B | 122M |
| 5 | Require token on TCP listener | 1.99B | 249M |

**Top 5 Goals: 38.2% of total tokens**

## Token Distribution by Role

| Role | % of Tokens | Avg Input per Task |
|------|-------------|-------------------|
| Develop | 74.8% | ~52.4M |
| Review | 4.5% | ~5.0M |
| PR | 1.1% | ~12.4M |
| Merge | 0.4% | ~2.7M |

## Key Insights

1. **Develop dominates**: 74.8% of tokens are used by develop agents
2. **Large tasks**: Average task uses ~119M tokens (unusually high)
3. **High cache rate**: 98.81% cache hit rate indicates efficient reuse
4. **Top tasks are expensive**: Top 10 tasks cost ~$4.3K (33.7% of total)

## Recommended Actions

### Immediate (1-2 weeks)
1. Implement prompt templates for common patterns
2. Add context summarization before sending to agents
3. Split top 5 high-cost tasks into smaller units
4. Audit large file handling in task descriptions

### Short-term (1 month)
1. Add pre-flight analysis before task creation
2. Implement shared context caching across related tasks
3. Optimize model selection based on task complexity
4. Add token budget alerts for oversized tasks

### Long-term (3 months)
1. Build context reduction pipeline
2. Implement task dependency analysis for context reuse
3. Create token usage dashboard for ongoing monitoring
4. Establish token budgets for different task types

## Expected Savings

By implementing recommended changes:
- **Potential reduction**: 30-50%
- **Estimated savings**: $6,400-$12,800 annually
- **Target avg tokens per task**: <50M (from ~119M)
- **Target top 10 costs**: <2.5K (from ~4.3K)

## Concrete Tasks for Optimization

For detailed concrete tasks with implementation roadmap, see [`token-usage-optimization-tasks.md`](token-usage-optimization-tasks.md).

The document outlines 10 priority tasks with:
- **High-priority tasks** addressing top cost drivers (1-2 weeks)
- **Medium-priority tasks** for systemic improvements (1 month)
- **Low-priority tasks** for secondary optimizations (3 months)

**Expected savings**: 30-50% token reduction (~$6,400-$12,800 annually)

## Detailed Analysis

For the complete token usage analysis with detailed breakdowns, case studies, and technical recommendations, see [`token-usage-analysis.md`](token-usage-analysis.md).
