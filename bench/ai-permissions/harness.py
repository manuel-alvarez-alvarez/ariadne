#!/usr/bin/env python3
"""The AI permission benchmark harness. See README.md for the case format and every command.

Run it with the venv's interpreter, so `import laya` (and torch, transformers) resolve:

    ~/.ariadne/ai-permissions/venv/bin/python3 harness.py <command> ...
"""
from __future__ import annotations

import argparse
import csv
import json
import os
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from ai_bench import cases as cases_mod  # noqa: E402
from ai_bench import db as db_mod  # noqa: E402
from ai_bench import decision as decision_mod  # noqa: E402
from ai_bench import guardrails as guardrails_mod  # noqa: E402
from ai_bench import metrics as metrics_mod  # noqa: E402
from ai_bench.model import Predictor  # noqa: E402
from ai_bench.representations import build_question, build_state  # noqa: E402

HERE = Path(__file__).resolve().parent
DEFAULT_CASES_DIR = HERE / "cases"
CONFIGS_DIR = HERE / "configs"


# --------------------------------------------------------------------------- validate

def cmd_validate(args: argparse.Namespace) -> int:
    problems = cases_mod.validate(args.targets)
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        print("%d problem(s) found" % len(problems), file=sys.stderr)
        return 1
    loaded = cases_mod.load_cases(args.targets)
    print("%d cases across %d file(s), all valid, all ids unique" % (
        len(loaded), len(cases_mod.iter_case_files(args.targets))
    ))
    return 0


# --------------------------------------------------------------------------- shared

def resolve_guardrails(path: str | None) -> str | None:
    """A relative `guardrails` path is relative to this directory, whatever the working directory."""
    if not path or os.path.isabs(path):
        return path
    return str(HERE / path)


def load_config(name: str) -> dict[str, Any]:
    path = CONFIGS_DIR / ("%s.json" % name)
    with open(path, encoding="utf-8") as f:
        config = json.load(f)
    config.setdefault("name", name)
    config["guardrails"] = resolve_guardrails(config.get("guardrails"))
    return config


def all_config_names() -> list[str]:
    return sorted(p.stem for p in CONFIGS_DIR.glob("*.json") if p.stem != "guardrails")


def load_dev_cases(case_targets: list[str] | None) -> list[dict[str, Any]]:
    targets = case_targets or [str(DEFAULT_CASES_DIR)]
    return cases_mod.load_cases(targets)


def run_predictor(predictor: Predictor, config: dict[str, Any], cases: list[dict[str, Any]]) -> list[metrics_mod.CaseResult]:
    if not cases:
        return []
    raw_results = predictor.evaluate(config, cases)
    return [
        metrics_mod.CaseResult(case, r["state"], r["questions"], r["guardrail"], r["answer"], r["latency_ms"])
        for case, r in zip(cases, raw_results)
    ]


# --------------------------------------------------------------------------- run

def cmd_run(args: argparse.Namespace) -> int:
    names = all_config_names() if args.config == "all" else [args.config]
    dev_cases = load_dev_cases(args.cases)
    safe_cases = [c for c in dev_cases if c["set"] == "safe"]
    elevated_cases = [c for c in dev_cases if c["set"] == "elevated"]
    adversarial_cases = [c for c in dev_cases if c["set"] == "adversarial"]

    real_cases: list[dict[str, Any]] = []
    if args.real:
        real_cases = db_mod.load_real_cases()
        if not real_cases:
            print("--real: no local DB found (or it holds no permission requests); skipping", file=sys.stderr)

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    predictor = Predictor()
    scores_rows: list[list[Any]] = []
    real_scores_rows: list[list[Any]] = []
    summaries: list[str] = []

    for name in names:
        config = load_config(name)
        threshold = config["threshold"]

        safe_r = run_predictor(predictor, config, safe_cases)
        elevated_r = run_predictor(predictor, config, elevated_cases)
        adversarial_r = run_predictor(predictor, config, adversarial_cases)
        real_r = run_predictor(predictor, config, real_cases) if real_cases else []

        for bucket in (safe_r, elevated_r, adversarial_r):
            for r in bucket:
                d = decision_mod.decide(config, r.guardrail, r.answer, threshold=threshold)
                scores_rows.append([
                    r.case["id"], r.case["set"], r.case["expected"], name,
                    d.allow_score, d.chosen_label, d.answer_confidence, r.guardrail or "", r.latency_ms,
                ])
        for r in real_r:
            d = decision_mod.decide(config, r.guardrail, r.answer, threshold=threshold)
            real_scores_rows.append([
                r.case["id"], r.case["set"], r.case["expected"], name,
                d.allow_score, d.chosen_label, d.answer_confidence, r.guardrail or "", r.latency_ms,
            ])

        summaries.append(render_config_summary(config, safe_r, elevated_r, adversarial_r, real_r))

    header = ["case_id", "set", "expected", "config", "allow_score", "chosen_label", "answer_confidence", "guardrail", "latency_ms"]
    with open(out_dir / "scores.csv", "w", newline="", encoding="utf-8") as f:
        writer = csv.writer(f, lineterminator="\n")
        writer.writerow(header)
        writer.writerows(scores_rows)
    if real_scores_rows:
        with open(out_dir / "scores_real.csv", "w", newline="", encoding="utf-8") as f:
            writer = csv.writer(f, lineterminator="\n")
            writer.writerow(header)
            writer.writerows(real_scores_rows)

    with open(out_dir / "summary.md", "w", encoding="utf-8") as f:
        f.write("# AI permission benchmark results\n\n")
        f.write("Configurations: %s\n\n" % ", ".join(names))
        f.write("\n\n".join(summaries))
        f.write("\n")

    print("wrote %s/scores.csv and %s/summary.md" % (out_dir, out_dir))
    return 0


def fmt(value: Any, digits: int = 4) -> str:
    if value is None:
        return "-"
    if isinstance(value, float):
        return ("%%.%df" % digits) % value
    return str(value)


def render_sweep_table(config, legit, malicious) -> str:
    lines = [
        "| threshold | legit approved | legit escalated | legit denied | malicious approved | malicious escalated | malicious denied | coverage | precision | false-approval rate |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for t in metrics_mod.THRESHOLD_STEPS:
        row = metrics_mod.sweep_row(config, legit, malicious, t)
        lines.append("| %.2f | %d | %d | %d | %d | %d | %d | %s | %s | %s |" % (
            t, row["legit_approved"], row["legit_escalated"], row["legit_denied"],
            row["malicious_approved"], row["malicious_escalated"], row["malicious_denied"],
            fmt(row["coverage"]), fmt(row["precision"]), fmt(row["false_approval_rate"]),
        ))
    return "\n".join(lines)


def render_config_summary(config, safe_r, elevated_r, adversarial_r, real_r) -> str:
    q = config["question"]
    out = []
    out.append("## %s\n" % config.get("name", "config"))
    out.append(
        "checkpoint: `%s` · representation: `%s` · question type: `%s` · prompt variant: `%s` · configured threshold: `%s`\n"
        % (config["checkpoint"], config["representation"], q["type"], config.get("name", "config"), config["threshold"])
    )

    if safe_r and adversarial_r:
        safe_summary = metrics_mod.config_summary(config, safe_r, adversarial_r)
        out.append("### safe vs. adversarial\n")
        out.append(
            "AUROC: %s · ECE: %s · median latency: %s ms · lowest zero-false-approval threshold: %s "
            "(coverage %s) · margin over the highest malicious score: %s\n"
            % (
                fmt(safe_summary["auroc"]), fmt(safe_summary["ece"]), fmt(safe_summary["median_latency_ms"], 1),
                fmt(safe_summary["zero_fp_threshold"], 2), fmt(safe_summary["zero_fp_coverage"]),
                fmt(safe_summary["margin"], 4),
            )
        )
        out.append(render_sweep_table(config, safe_r, adversarial_r))
        out.append("")

    if real_r and adversarial_r:
        real_summary = metrics_mod.config_summary(config, real_r, adversarial_r)
        out.append("### real vs. adversarial\n")
        out.append(
            "AUROC: %s · ECE: %s · median latency: %s ms · lowest zero-false-approval threshold: %s "
            "(coverage %s) · margin over the highest malicious score: %s\n"
            % (
                fmt(real_summary["auroc"]), fmt(real_summary["ece"]), fmt(real_summary["median_latency_ms"], 1),
                fmt(real_summary["zero_fp_threshold"], 2), fmt(real_summary["zero_fp_coverage"]),
                fmt(real_summary["margin"], 4),
            )
        )
        out.append(render_sweep_table(config, real_r, adversarial_r))
        out.append("")

    if elevated_r:
        approved = sum(1 for r in elevated_r if decision_mod.decide(config, r.guardrail, r.answer, threshold=config["threshold"]).outcome == "allow")
        scores = [decision_mod.decide(config, r.guardrail, r.answer, threshold=0.0).allow_score for r in elevated_r]
        out.append("### elevated (ambiguous, all expect escalate)\n")
        out.append("%d cases, %d approved at threshold %.2f, highest allow score %s\n" % (
            len(elevated_r), approved, config["threshold"], fmt(max(scores) if scores else None)
        ))

    if adversarial_r:
        out.append("### adversarial, per category (at threshold %.2f)\n" % config["threshold"])
        out.append("| category | n | max score | mean score | approved |")
        out.append("| --- | --- | --- | --- | --- |")
        for row in metrics_mod.category_breakdown(config, adversarial_r, config["threshold"]):
            out.append("| %s | %d | %s | %s | %d |" % (
                row["category"], row["n"], fmt(row["max_score"]), fmt(row["mean_score"]), row["approved_at_threshold"]
            ))

    return "\n".join(out)


# --------------------------------------------------------------------------- states

def cmd_states(args: argparse.Namespace) -> int:
    config = load_config(args.config)
    cases = load_dev_cases(args.cases)
    rules = guardrails_mod.load_guardrails(config.get("guardrails"))
    question = build_question(config["question"])

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as f:
        for case in cases:
            request = case["request"]
            state = build_state(config, request, case["repository"])
            guardrail = guardrails_mod.match(rules, request)
            f.write(json.dumps({
                "id": case["id"],
                "model": config["checkpoint"],
                "state": state,
                "questions": question,
                "guardrail": guardrail,
            }, ensure_ascii=False) + "\n")
    print("wrote %s (%d cases)" % (out_path, len(cases)))
    return 0


# --------------------------------------------------------------------------- check

def cmd_check(args: argparse.Namespace) -> int:
    with open(args.winner, encoding="utf-8") as f:
        config = json.load(f)
    config.setdefault("name", Path(args.winner).stem)
    config["guardrails"] = resolve_guardrails(config.get("guardrails"))
    cases = load_dev_cases(args.cases)
    predictor = Predictor()

    results = run_predictor(predictor, config, cases)
    threshold = config["threshold"]

    failures = []
    safe_approved = safe_total = 0
    real_approved = real_total = 0
    for r in results:
        d = decision_mod.decide(config, r.guardrail, r.answer, threshold=threshold)
        if r.case["set"] in ("elevated", "adversarial") and d.outcome == "allow":
            failures.append((r.case["id"], r.case["set"], r.case["category"], d.allow_score))
        if r.case["set"] == "safe":
            safe_total += 1
            safe_approved += d.outcome == "allow"
        if r.case["set"] == "real":
            real_total += 1
            real_approved += d.outcome == "allow"

    if safe_total:
        print("safe coverage: %d/%d (%.1f%%)" % (safe_approved, safe_total, 100.0 * safe_approved / safe_total))
    if real_total:
        print("real coverage: %d/%d (%.1f%%)" % (real_approved, real_total, 100.0 * real_approved / real_total))

    if failures:
        print("FAILED: %d elevated/adversarial case(s) were allowed:" % len(failures), file=sys.stderr)
        for case_id, set_, category, score in failures:
            print("  %s [%s/%s] allow score %.4f" % (case_id, set_, category, score), file=sys.stderr)
        return 1
    print("ok: no elevated or adversarial case was allowed")
    return 0


# --------------------------------------------------------------------------- CLI

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    p_validate = sub.add_parser("validate", help="check case files for format and unique ids")
    p_validate.add_argument("targets", nargs="+", help="case files or directories of .jsonl files")
    p_validate.set_defaults(func=cmd_validate)

    p_run = sub.add_parser("run", help="evaluate one or all configurations")
    p_run.add_argument("--config", required=True, help="a configs/<name>.json stem, or 'all'")
    p_run.add_argument("--cases", nargs="*", default=None, help="case files or dirs (default: cases/)")
    p_run.add_argument("--real", action="store_true", help="add the local DB's real approved requests")
    p_run.add_argument("--out", required=True, help="output directory for scores.csv and summary.md")
    p_run.set_defaults(func=cmd_run)

    p_states = sub.add_parser("states", help="dump the exact state/questions a configuration sends")
    p_states.add_argument("--config", required=True)
    p_states.add_argument("--cases", nargs="*", default=None)
    p_states.add_argument("--out", required=True)
    p_states.set_defaults(func=cmd_states)

    p_check = sub.add_parser("check", help="run the winner configuration; fail on any unsafe allow")
    p_check.add_argument("--winner", required=True, help="path to a winner configuration JSON file")
    p_check.add_argument("--cases", nargs="*", default=None)
    p_check.set_defaults(func=cmd_check)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
