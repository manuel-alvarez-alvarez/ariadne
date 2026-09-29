#!/usr/bin/env python3
"""Run AI permission benchmark evaluators against JSON Lines cases.

Evaluators live under `evaluators/`, one package per backend, and register under a unique
key; `list` prints them and `run --evaluator <key>` runs one: `setup`, `evaluate` per case,
then `teardown`.
"""
from __future__ import annotations

import argparse
import csv
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from ai_bench import cases as cases_mod
from ai_bench import db as db_mod
from ai_bench import metrics, registry
from ai_bench.derive import RULES, TAGS, derive
from ai_bench.evaluator import EvaluationResult, Evaluator, EvaluatorError

HERE = Path(__file__).resolve().parent
DEFAULT_CASES = [HERE / "cases" / name for name in ("safe.jsonl", "elevated.jsonl", "adversarial-dev.jsonl")]
HELDOUT_CASES = [HERE / "cases" / name for name in ("safe-heldout.jsonl", "adversarial-heldout.jsonl")]
RUNS = HERE / "out" / "runs"
DEFAULT_MARGIN = 0.05


def evaluate_all(evaluator: Evaluator, cases: list[dict[str, Any]]) -> list[EvaluationResult]:
    """`setup`, one `evaluate` per case, timed, then `teardown` however the run ends."""
    evaluator.setup()
    try:
        results = []
        for case in cases:
            started = time.perf_counter()
            evaluation = evaluator.evaluate(case)
            latency_ms = (time.perf_counter() - started) * 1000.0
            if evaluation.label not in ("allow", "ask", "deny"):
                raise EvaluatorError("%s: invalid label %r for %s" % (evaluator.key, evaluation.label, case["id"]))
            results.append(EvaluationResult(case["id"], evaluation.danger, evaluation.label, latency_ms))
        return results
    finally:
        evaluator.teardown()


def run_case_files(targets: list[str] | None, heldout: bool = False) -> list[Path]:
    """The development cases, or the held-out ones with `heldout`; named `targets` instead,
    plus the held-out ones with `heldout`. A directory never contributes its held-out files."""
    if targets is None:
        return HELDOUT_CASES if heldout else DEFAULT_CASES
    files = []
    for target in targets:
        path = Path(target)
        if path.is_dir():
            files.extend(file for file in sorted(path.glob("*.jsonl")) if not file.name.endswith("-heldout.jsonl"))
        else:
            files.append(path)
    return files + (HELDOUT_CASES if heldout else [])


def format_value(value: float | int | None) -> str:
    if value is None:
        return "-"
    if isinstance(value, float):
        return "%.4f" % value
    return str(value)


def format_shares(shares: dict[str, float | None]) -> str:
    return "/".join(format_value(shares[label]) for label in ("allow", "ask", "deny"))


def print_table(rows: list[tuple[str, dict[str, Any]]], include_real: bool) -> None:
    columns = [
        ("evaluator", "evaluator"),
        ("safe a/k/d", "safe"),
        ("elevated a/k/d", "elevated"),
        ("adversarial a/k/d", "adversarial"),
    ]
    if include_real:
        columns.append(("real a/k/d", "real"))
    columns += [
        ("risky allowed", "risky_allowed"),
        ("safe denied", "safe_denied"),
        ("AUROC risky", "auroc_risky"),
        ("AUROC deny", "auroc_deny"),
        ("accuracy", "accuracy"),
        ("median latency ms", "median_latency_ms"),
    ]
    rendered = []
    for name, summary in rows:
        row = [name]
        for _, key in columns[1:]:
            value = summary[key]
            row.append(format_shares(value) if isinstance(value, dict) else format_value(value))
        rendered.append(row)
    widths = [len(title) for title, _ in columns]
    for row in rendered:
        widths = [max(width, len(value)) for width, value in zip(widths, row)]
    print("  ".join(title.ljust(width) for (title, _), width in zip(columns, widths)))
    print("  ".join("-" * width for width in widths))
    for row in rendered:
        print("  ".join(value.ljust(width) for value, width in zip(row, widths)))


def print_selection(name: str, cases: list[dict[str, Any]], results: list[EvaluationResult], margin: float) -> None:
    print()
    print("%s selection (margin %.2f)" % (name, margin))
    try:
        selection = metrics.select_thresholds(cases, results, margin)
    except ValueError as exc:
        print("  %s" % exc)
        return
    if selection["has_pair"]:
        print(
            "  pair: allow_threshold %.4f / deny_threshold %.4f"
            % (selection["allow_threshold"], selection["deny_threshold"])
        )
    else:
        print(
            "  no pair: allow_threshold %.4f is not under deny_threshold %.4f"
            % (selection["allow_threshold"], selection["deny_threshold"])
        )
    print("  nearest the allow bound:")
    for case_id, danger in selection["nearest_allow"]:
        print("    %-40s %.4f" % (case_id, danger))
    print("  nearest the deny bound:")
    for case_id, danger in selection["nearest_deny"]:
        print("    %-40s %.4f" % (case_id, danger))
    relabelled = [
        metrics.at_thresholds(result, selection["allow_threshold"], selection["deny_threshold"]) for result in results
    ]
    print("  table at that pair:")
    include_real = any(case["set"] == "real" for case in cases)
    print_table([(name, metrics.summary(cases, relabelled))], include_real)


def write_scores(directory: Path, name: str, cases: list[dict[str, Any]], results: list[EvaluationResult]) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    safe_name = "".join(character if character.isalnum() or character in "._-" else "_" for character in name)
    with (directory / (safe_name + ".csv")).open("w", newline="", encoding="utf-8") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(["id", "set", "expected", "danger", "label", "latency_ms"])
        writer.writerows(
            [
                case["id"],
                case["set"],
                case["expected"],
                result.danger,
                result.label,
                result.latency_ms,
            ]
            for case, result in zip(cases, results)
        )


def read_scores(path: Path) -> tuple[list[dict[str, Any]], list[EvaluationResult]]:
    """Read one per-case CSV that `write_scores` wrote back into cases and results."""
    cases = []
    results = []
    try:
        with path.open(newline="", encoding="utf-8") as scores:
            for row in csv.DictReader(scores):
                if row["label"] not in ("allow", "ask", "deny"):
                    raise EvaluatorError("%s: invalid label %r for %s" % (path, row["label"], row["id"]))
                cases.append({"id": row["id"], "set": row["set"], "expected": row["expected"]})
                results.append(
                    EvaluationResult(
                        row["id"],
                        float(row["danger"]) if row["danger"] else None,
                        row["label"],
                        float(row["latency_ms"]),
                    )
                )
    except OSError as exc:
        raise EvaluatorError("could not read scores %s: %s" % (path, exc)) from exc
    except KeyError as exc:
        raise EvaluatorError("%s is missing the %s column" % (path, exc)) from exc
    return cases, results


def score_files(targets: list[str]) -> list[Path]:
    files = []
    for target in targets:
        path = Path(target)
        files.extend(sorted(path.glob("*.csv")) if path.is_dir() else [path])
    if not files:
        raise EvaluatorError("no score files in %s" % ", ".join(targets))
    return files


def cmd_validate(args: argparse.Namespace) -> int:
    problems = cases_mod.validate(args.targets)
    if problems:
        print("\n".join(problems), file=sys.stderr)
        return 1
    print("%d cases across %d file(s), all valid, all ids unique" % (len(cases_mod.load_cases(args.targets)), len(cases_mod.iter_case_files(args.targets))))
    return 0


def cmd_list(args: argparse.Namespace) -> int:
    registered = registry.load_all()
    if args.tsv:
        for key, cls in registered.items():
            print("%s\t%s" % (key, cls.backend))
        return 0
    width = max((len(key) for key in registered), default=3)
    for key, cls in registered.items():
        print("%s  %-4s  %s" % (key.ljust(width), cls.backend, cls.description))
    return 0


def cmd_run(args: argparse.Namespace) -> int:
    evaluators = [registry.get(key) for key in args.evaluator]
    cases = cases_mod.load_cases(run_case_files(args.cases, args.heldout))
    if args.real:
        cases.extend(db_mod.load_real_cases())
    out = Path(args.out) if args.out else RUNS / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    summaries = []
    runs = []
    for cls in evaluators:
        results = evaluate_all(cls(), cases)
        summaries.append((cls.key, metrics.summary(cases, results)))
        runs.append((cls.key, cases, results))
        write_scores(out, cls.key, cases, results)
    print_table(summaries, args.real)
    if args.select:
        for name, run_cases, results in runs:
            print_selection(name, run_cases, results, args.margin)
    print("\nper-case scores: %s" % out)
    return 0


def cmd_derive(args: argparse.Namespace) -> int:
    """Print the cases on which each deterministic fact and hard rule fires."""
    cases = cases_mod.load_cases(run_case_files(args.cases, args.heldout))
    if args.real:
        cases.extend(db_mod.load_real_cases())
    derived_cases = [(case, derive(case["request"], case.get("repository"))) for case in cases]
    sets = []
    for case in cases:
        if case["set"] not in sets:
            sets.append(case["set"])
    rows = []
    rule_ids: dict[str, list[str]] = {rule: [] for rule in RULES}
    for kind, names in (("tag", TAGS), ("rule", RULES)):
        for name in names:
            counts = dict.fromkeys(sets, 0)
            for case, result in derived_cases:
                fired = name in result.risk_tags if kind == "tag" else result.rule == name
                if fired:
                    counts[case["set"]] += 1
                    if kind == "rule":
                        rule_ids[name].append(case["id"])
            rows.append([kind, name, *(str(counts[set_name]) for set_name in sets)])
    headers = ["kind", "name", *sets]
    widths = [len(header) for header in headers]
    for row in rows:
        widths = [max(width, len(value)) for width, value in zip(widths, row)]
    print("  ".join(value.ljust(width) for value, width in zip(headers, widths)))
    print("  ".join("-" * width for width in widths))
    for row in rows:
        print("  ".join(value.ljust(width) for value, width in zip(row, widths)))
    print("rule ids:")
    for rule in RULES:
        print("  %s: %s" % (rule, ", ".join(rule_ids[rule]) or "-"))
    return 0


def cmd_report(args: argparse.Namespace) -> int:
    summaries = []
    runs = []
    include_real = False
    for path in score_files(args.targets):
        cases, results = read_scores(path)
        include_real = include_real or any(case["set"] == "real" for case in cases)
        summaries.append((path.stem, metrics.summary(cases, results)))
        runs.append((path.stem, cases, results))
    print_table(summaries, include_real)
    return 0


def cmd_select(args: argparse.Namespace) -> int:
    for path in score_files(args.targets):
        cases, results = read_scores(path)
        print_selection(path.stem, cases, results, args.margin)
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    validate = subcommands.add_parser("validate", help="validate JSON Lines case files")
    validate.add_argument("targets", nargs="+", help="case files or directories")
    validate.set_defaults(func=cmd_validate)
    listing = subcommands.add_parser("list", help="list the registered evaluators: key, backend, description")
    listing.add_argument("--tsv", action="store_true", help="print key and backend only, tab separated")
    listing.set_defaults(func=cmd_list)
    run = subcommands.add_parser("run", help="run one or more evaluators")
    run.add_argument("--evaluator", action="append", required=True, help="an evaluator key from `list`; repeat for each evaluator")
    run.add_argument("--cases", nargs="+", help="case files or directories (default: the development cases); a directory never adds its held-out files")
    run.add_argument("--heldout", action="store_true", help="run the held-out cases: instead of the development ones, or after --cases")
    run.add_argument("--real", action="store_true", help="also load approved cases from ~/.ariadne/ariadne.db read-only")
    run.add_argument("--select", action="store_true", help="print each evaluator's threshold selection after the table")
    run.add_argument("--margin", type=float, default=DEFAULT_MARGIN, help="selection margin (default: %s)" % DEFAULT_MARGIN)
    run.add_argument("--out", help="directory for the per-case CSVs (default: out/runs/<UTC time>)")
    run.set_defaults(func=cmd_run)
    derived = subcommands.add_parser("derive", help="report deterministic risk tags and hard rules")
    derived.add_argument("--cases", nargs="+", help="case files or directories (default: the development cases); a directory never adds its held-out files")
    derived.add_argument("--heldout", action="store_true", help="use the held-out cases, or add them after --cases")
    derived.add_argument("--real", action="store_true", help="also load approved cases from ~/.ariadne/ariadne.db read-only")
    derived.set_defaults(func=cmd_derive)
    report = subcommands.add_parser("report", help="print one table from per-case CSV files that earlier runs wrote")
    report.add_argument("targets", nargs="+", help="CSV files or directories of them")
    report.set_defaults(func=cmd_report)
    select = subcommands.add_parser("select", help="print each evaluator's threshold selection from per-case CSV files")
    select.add_argument("targets", nargs="+", help="CSV files or directories of them")
    select.add_argument("--margin", type=float, default=DEFAULT_MARGIN, help="selection margin (default: %s)" % DEFAULT_MARGIN)
    select.set_defaults(func=cmd_select)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except (EvaluatorError, cases_mod.CaseError, ValueError) as exc:
        print("error: %s" % exc, file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
