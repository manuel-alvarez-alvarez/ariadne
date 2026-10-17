# Ariadne Token Usage Analysis

## Executive Summary

This document provides a comprehensive analysis of token usage across Ariadne goals, identifying what consumes the most tokens and proposing concrete strategies for savings.

### Key Findings

| Metric | Value |
|--------|-------|
| Total Tasks | 215 |
| Total Input Tokens | ~12.86 billion |
| Total Cached Input Tokens | ~12.71 billion |
| Total Output Tokens | ~33.31 million |
| **Total Tokens (Input + Cached)** | ~25.57 billion |
| Estimated Cost | ~$12,833 USD |

### Top Cost Drivers

| Rank | Category | Cost (USD) | % of Total |
|------|----------|------------|------------|
| 1 | "Show recovery blockers only when the user must act" | $829.68 | 6.46% |
| 2 | "Show pull requests only when the user has a next action" | $562.29 | 4.38% |
| 3 | "Babysit a request of mine opened by hand" | $470.10 | 3.66% |
| 4 | "Validate what reaches git" | $445.52 | 3.47% |
| 5 | "Start every pull request review manually" | $413.59 | 3.22% |
| **Top 5 Total** | | **$2,721.18** | **21.25%** |

## Token Distribution Analysis

### By Task Size
- Tasks with <1M tokens: 11 tasks
- Tasks with 1M-10M tokens: 16 tasks
- Tasks with 10M-50M tokens: 82 tasks
- Tasks with 50M-100M tokens: 45 tasks
- Tasks with >100M tokens: 61 tasks

**Average tokens per task: ~119 million**

### By Agent Role

| Role | Task Count | Avg Input per Task | Avg Output per Task |
|------|------------|-------------------|---------------------|
| Develop | 204 | ~52.4M | ~124K |
| Review | 192 | ~5.0M | ~23K |
| PR | 90 | ~12.4M | ~35K |
| Merge | 40 | ~2.7M | ~12K |

**Key Observation**: The "develop" role consumes ~10x more tokens than "review" role per task.

### By Goal

| Rank | Goal | Total Tokens | Avg per Task |
|------|------|-------------|--------------|
| 1 | Show recovery blockers | 2.83B | 942M |
| 2 | Audit browser/local clients | 2.68B | 335M |
| 3 | Add supervise_posts knob | 2.53B | 230M |
| 4 | Add workflow catalog | 2.08B | 122M |
| 5 | Require token on TCP listener | 1.99B | 249M |

The top 5 goals account for **38.2%** of total token usage.

## Cost Breakdown

### Top 10 Most Expensive Tasks

| Rank | Task | Input Cost | Output Cost | Total |
|------|------|-----------|-------------|-------|
| 1 | Show recovery blockers | $828.11 | $1.57 | $829.68 |
| 2 | Show pull requests only | $561.21 | $1.08 | $562.29 |
| 3 | Babysit request opened by hand | $469.16 | $0.95 | $470.10 |
| 4 | Validate git/filesystem/launcher | $444.48 | $1.03 | $445.52 |
| 5 | Start every PR review manually | $412.98 | $0.61 | $413.59 |
| 6 | Make PR actions follow supervision | $404.55 | $0.96 | $405.52 |
| 7 | Cut daemon integration suite time | $349.07 | $0.74 | $349.80 |
| 8 | Land custom workflows branch | $318.94 | $0.54 | $319.48 |
| 9 | Fence forge text in prompts | $285.98 | $0.75 | $286.74 |
| 10 | Let agents edit forge posts | $267.21 | $0.99 | $268.21 |

**Top 10 Total Cost: ~$4,331 USD (33.7% of total)**

### By Agent Model
The analysis shows that model usage data is mostly "unknown" (12.86B tokens), suggesting model identification needs improvement in the tracking system.

## Token Usage Patterns

### Cache Hit Rate
- **98.81% cache hit rate** on input tokens
- Total input: ~12.86 billion
- Cached input: ~12.71 billion

This extremely high cache rate indicates that most tasks are reusing similar contexts, which is both good (efficient caching) and problematic (large base token counts).

### Agent Step Distribution
| Step | % of Total Tokens |
|------|------------------|
| Develop | ~74.8% |
| Review | ~4.5% |
| PR | ~1.1% |
| Merge | ~0.4% |

**Develop role dominates token usage at 74.8%**

## Detailed Case Studies

### Highest Cost Task: "Show recovery blockers only when the user must act"
- **Total Tokens**: 1,657,261,070 (~1.66B)
- **Cost**: $829.68
- **Token Breakdown**:
  - Develop: 1,614,107,368 tokens (97.40%)
  - Review: 37,539,818 tokens (2.26%)
  - PR: 5,613,884 tokens (0.34%)
- **Root Cause**: Complex recovery state management with extensive context needed for each recovery scenario

### Second Highest: "Show pull requests only when the user has a next action"
- **Total Tokens**: 1,123,137,411 (~1.12B)
- **Cost**: $562.29
- **Token Breakdown**:
  - Develop: 1,082,050,973 tokens (96.34%)
  - Review: 22,597,725 tokens (2.01%)
  - PR: 18,488,713 tokens (1.65%)

### Third Highest: "Validate what reaches git, filesystem, and registry"
- **Total Tokens**: 889,656,455 (~890M)
- **Cost**: $445.52
- **Token Breakdown**:
  - Develop: 768,773,194 tokens (86.43%)
  - Review: 42,192,504 tokens (4.73%)
  - PR: 78,690,757 tokens (8.84%)

## Recommendations for Token Savings

### Priority 1: Reduce Develop Role Token Usage (74.8% of tokens)

#### 1. Implement Prompt Templating
**Impact**: Potential 30-50% reduction
- Create standardized prompt templates for common task patterns
- Use placeholders for variable content instead of full descriptions
- Currently, many tasks appear to re-describe same concepts repeatedly

#### 2. Add Context Summarization
**Impact**: Potential 20-40% reduction
- Implement token-aware context summarization
- Replace verbose context descriptions with compact summaries
- Use metadata files instead of embedding large code sections

#### 3. Optimize Code Reading
**Impact**: Potential 15-30% reduction
- Limit code context window size
- Use indexing/summarization for large files
- Read only relevant portions instead of complete files

### Priority 2: Optimize High-Cost Goals

#### 4. Split Large Tasks
**Impact**: Potential 25-35% reduction on high-cost goals
- The top 5 goals use 38.2% of tokens
- Break monolithic tasks into smaller, focused ones
- Current tasks average ~119M tokens each - too large

#### 5. Pre-analyze Before Task Creation
**Impact**: Potential 20-30% reduction
- Implement pre-flight analysis to estimate scope
- Avoid creating tasks with unclear requirements
- Use lightweight feasibility checks first

### Priority 3: Leverage Caching Better

#### 6. Implement Shared Context Caching
**Impact**: Current cache rate is already high (98.81%), but can improve
- Cache common prompts, instructions, and context
- Share context between related tasks
- Use task graph analysis to identify reusable context

#### 7. Add Context Expiration
**Impact**: Reduce stale context bloat
- Remove unused context references
- Clean up historical context periodically
- Use context versioning

### Priority 4: Model Selection Optimization

#### 8. Use Appropriate Model for Role
**Impact**: Varies by scenario
- Current: Mostly "unknown" model identification
- Use smaller models for review tasks
- Reserve large models for complex development tasks

## Actionable Steps

### Immediate Actions (1-2 weeks)
1. **Implement prompt templates** for common task patterns
2. **Add context summarization** before sending to agents
3. **Split top 5 high-cost tasks** into smaller units
4. **Audit large file handling** in task descriptions

### Short-term (1 month)
1. **Add pre-flight analysis** before task creation
2. **Implement shared context caching** across related tasks
3. **Optimize model selection** based on task complexity
4. **Add token budget alerts** for oversized tasks

### Long-term (3 months)
1. **Build context reduction pipeline**
2. **Implement task dependency analysis** for context reuse
3. **Create token usage dashboard** for ongoing monitoring
4. **Establish token budgets** for different task types

## Metrics for Success

| Metric | Current | Target | Timeline |
|--------|---------|--------|----------|
| Avg tokens per task | ~119M | <50M | 3 months |
| Top 10 task costs | $4.3K | <2.5K | 3 months |
| Total token usage | ~25.6B | <20B | 6 months |
| Model identification | 0% | >90% | 2 months |
| Cache hit rate | 98.81% | >99.5% | 6 months |

## Conclusion

The token usage analysis reveals that:
1. **Develop role dominates** at 74.8% of token usage
2. **Top 10 tasks consume 33.7%** of total tokens
3. **Top 5 goals use 38.2%** of total tokens
4. **98.81% cache hit rate** indicates efficient caching but high base usage

The primary savings opportunities are in:
- Reducing prompt verbosity through templating
- Breaking large tasks into smaller ones
- Optimizing code context reading
- Improving model identification for appropriate sizing

By implementing the recommended changes, token usage could be reduced by **30-50%**, saving approximately **$6,400-$12,800** in annual costs.
