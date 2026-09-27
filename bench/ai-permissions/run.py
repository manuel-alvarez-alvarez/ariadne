#!/usr/bin/env python3
"""Run AI permission benchmark evaluators against JSON Lines cases."""
from __future__ import annotations

import argparse
import csv
import json
import sys
from pathlib import Path
from typing import Any

from ai_bench import cases as cases_mod
from ai_bench import db as db_mod
from ai_bench import metrics
from ai_bench.evaluator import EvaluationResult, EvaluatorError, make_evaluator

HERE = Path(__file__).resolve().parent
CONFIGS = HERE / "configs"
DEFAULT_CASES = [HERE / "cases" / name for name in ("safe.jsonl", "elevated.jsonl", "adversarial-dev.jsonl")]
DEFAULT_OUT = HERE / "out"


def load_config(path: Path | None) -> dict[str, Any] | None:
    if path is None:
        return None
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise EvaluatorError("could not read config %s: %s" % (path, exc)) from exc
    except json.JSONDecodeError as exc:
        raise EvaluatorError("invalid config %s: %s" % (path, exc)) from exc
    if not isinstance(value, dict):
        raise EvaluatorError("config %s must be a JSON object" % path)
    return value


def evaluator_spec(value: str) -> tuple[str, Path | None, str]:
    name, separator, configured_path = value.partition("=")
    if not name:
        raise EvaluatorError("evaluator name cannot be empty")
    if separator:
        path = Path(configured_path)
        return name, path, "%s=%s" % (name, path)
    default = CONFIGS / (name + ".json")
    if default.exists():
        return name, default, "%s=configs/%s.json" % (name, name)
    return name, None, name


def run_case_files(targets: list[str] | None) -> list[Path]:
    if targets is None:
        return DEFAULT_CASES
    files = []
    for target in targets:
        path = Path(target)
        if path.is_dir():
            files.extend(file for file in sorted(path.glob("*.jsonl")) if not file.name.endswith("-heldout.jsonl"))
        else:
            files.append(path)
    return files


def format_value(value: float | int | None) -> str:
    if value is None:
        return "-"
    if isinstance(value, float):
        return "%.4f" % value
    return str(value)


def print_table(rows: list[tuple[str, dict[str, float | int | None]]], include_real: bool) -> None:
    columns = [
        ("evaluator", "evaluator"),
        ("safe coverage", "safe_coverage"),
        ("elevated approvals", "elevated_approvals"),
        ("adversarial approvals", "adversarial_approvals"),
        ("AUROC", "auroc"),
        ("precision", "precision"),
        ("recall", "recall"),
        ("F1", "f1"),
        ("accuracy", "accuracy"),
        ("median latency ms", "median_latency_ms"),
    ]
    if include_real:
        columns.append(("real coverage", "real_coverage"))
    rendered = [[name] + [format_value(summary[key]) for _, key in columns[1:]] for name, summary in rows]
    widths = [len(title) for title, _ in columns]
    for row in rendered:
        widths = [max(width, len(value)) for width, value in zip(widths, row)]
    print("  ".join(title.ljust(width) for (title, _), width in zip(columns, widths)))
    print("  ".join("-" * width for width in widths))
    for row in rendered:
        print("  ".join(value.ljust(width) for value, width in zip(row, widths)))


def print_sweeps(runs: list[tuple[str, list[dict[str, Any]], list[EvaluationResult]]]) -> None:
    for name, cases, results in runs:
        print()
        print("%s threshold sweep" % name)
        print("threshold  safe coverage  elevated approvals  adversarial approvals  precision  recall  F1  accuracy")
        for threshold in metrics.THRESHOLD_STEPS:
            row = metrics.summary(cases, [metrics.at_threshold(result, threshold) for result in results])
            print(
                "%.2f       %s  %s                  %s                     %s  %s  %s  %s"
                % (
                    threshold,
                    format_value(row["safe_coverage"]),
                    format_value(row["elevated_approvals"]),
                    format_value(row["adversarial_approvals"]),
                    format_value(row["precision"]),
                    format_value(row["recall"]),
                    format_value(row["f1"]),
                    format_value(row["accuracy"]),
                )
            )


def write_scores(directory: Path, name: str, cases: list[dict[str, Any]], results: list[EvaluationResult]) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    safe_name = "".join(character if character.isalnum() or character in "._-" else "_" for character in name)
    with (directory / (safe_name + ".csv")).open("w", newline="", encoding="utf-8") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(["id", "set", "expected", "allow_score", "label", "latency_ms"])
        writer.writerows(
            [
                case["id"],
                case["set"],
                case["expected"],
                result.allow_score,
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
                if row["label"] not in ("allow", "escalate"):
                    raise EvaluatorError("%s: invalid label %r for %s" % (path, row["label"], row["id"]))
                cases.append({"id": row["id"], "set": row["set"], "expected": row["expected"]})
                results.append(
                    EvaluationResult(
                        row["id"],
                        float(row["allow_score"]) if row["allow_score"] else None,
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


def cmd_run(args: argparse.Namespace) -> int:
    cases = cases_mod.load_cases(run_case_files(args.cases))
    if args.real:
        cases.extend(db_mod.load_real_cases())
    summaries = []
    runs = []
    for value in args.evaluator:
        name, config_path, display_name = evaluator_spec(value)
        evaluator = make_evaluator(name)
        results = evaluator.evaluate(load_config(config_path), cases)
        if len(results) != len(cases):
            raise EvaluatorError("%s returned %d results for %d cases" % (name, len(results), len(cases)))
        summaries.append((display_name, metrics.summary(cases, results)))
        runs.append((display_name, cases, results))
        write_scores(Path(args.out), display_name, cases, results)
    print_table(summaries, args.real)
    if args.sweep:
        print_sweeps(runs)
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
    if args.sweep:
        print_sweeps(runs)
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    validate = subcommands.add_parser("validate", help="validate JSON Lines case files")
    validate.add_argument("targets", nargs="+", help="case files or directories")
    validate.set_defaults(func=cmd_validate)
    run = subcommands.add_parser("run", help="run one or more evaluators")
    run.add_argument("--evaluator", action="append", required=True, help="name or name=config.json; repeat for each evaluator")
    run.add_argument("--cases", nargs="+", help="case files or directories; held-out files must be named directly")
    run.add_argument("--real", action="store_true", help="also load approved cases from ~/.ariadne/ariadne.db read-only")
    run.add_argument("--sweep", action="store_true", help="print the 0.00 to 1.00 score threshold sweep")
    run.add_argument("--out", default=str(DEFAULT_OUT), help="directory for per-case CSV output (default: %(default)s)")
    run.set_defaults(func=cmd_run)
    report = subcommands.add_parser("report", help="print one table from per-case CSV files that earlier runs wrote")
    report.add_argument("targets", nargs="+", help="CSV files or directories of them")
    report.add_argument("--sweep", action="store_true", help="print the 0.00 to 1.00 score threshold sweep")
    report.set_defaults(func=cmd_report)
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
