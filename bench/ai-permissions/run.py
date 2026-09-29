#!/usr/bin/env python3
"""Run AI permission benchmark evaluators against JSON Lines cases.

Evaluators live under `evaluators/`, one package per backend, and register under a unique
key; `list` prints them and `run --evaluator <key>` runs one: `setup`, `evaluate` per case,
then `teardown`. `fixture --evaluator <key>` prints what a mode sends for each case, and what
its rules and caps decide, with no model.
"""
from __future__ import annotations

import argparse
import csv
import importlib
import json
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from ai_bench import cases as cases_mod
from ai_bench import db as db_mod
from ai_bench import metrics, probe, registry, representations
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
            results.append(
                EvaluationResult(
                    case["id"], evaluation.danger, evaluation.label, latency_ms, evaluation.rule, evaluation.cap
                )
            )
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
        ("dangerous auto-allow", "dangerous_auto_allow_rate"),
        ("benign auto-allow", "benign_auto_allow_rate"),
        ("ask rate", "ask_rate"),
        ("false deny", "false_deny_rate"),
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
    if selection["decided"]:
        print("  risky cases that a rule or a cap decides, off the allow bound: %d" % len(selection["decided"]))
    if selection["rule_denied"]:
        print("  broken hard rule, a rule denies %d safe or real case(s):" % len(selection["rule_denied"]))
        for case_id in selection["rule_denied"]:
            print("    %s" % case_id)
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


def print_by_table(by: str, rows: dict[str, dict[str, Any]]) -> None:
    columns = [(by, "key"), ("count", "count"), ("a/k/d", "shares"), ("risky allowed", "risky_allowed"), ("safe denied", "safe_denied")]
    rendered = []
    for key in sorted(rows):
        row = rows[key]
        rendered.append(
            [
                key,
                str(row["count"]),
                format_shares(row["shares"]),
                format_value(row["risky_allowed"]),
                format_value(row["safe_denied"]),
            ]
        )
    widths = [len(title) for title, _ in columns]
    for row in rendered:
        widths = [max(width, len(value)) for width, value in zip(widths, row)]
    print("  ".join(title.ljust(width) for (title, _), width in zip(columns, widths)))
    print("  ".join("-" * width for width in widths))
    for row in rendered:
        print("  ".join(value.ljust(width) for value, width in zip(row, widths)))


def print_pair_table(pairs: dict[str, Any]) -> None:
    print("  pairs: %d" % pairs["pairs"])
    print("  correct: %d" % pairs["correct"])
    if pairs["incorrect"]:
        print("  incorrect:")
        for one, other in pairs["incorrect"]:
            print("    %s / %s" % (one, other))


def print_grouping(by: str, name: str, cases: list[dict[str, Any]], results: list[EvaluationResult]) -> None:
    print()
    print(name)
    if by == "pair":
        print_pair_table(metrics.by_pair(cases, results))
    else:
        grouping = metrics.by_operation if by == "operation" else metrics.by_tag
        print_by_table(by, grouping(cases, results))


def write_scores(directory: Path, name: str, cases: list[dict[str, Any]], results: list[EvaluationResult]) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    safe_name = "".join(character if character.isalnum() or character in "._-" else "_" for character in name)
    with (directory / (safe_name + ".csv")).open("w", newline="", encoding="utf-8") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(
            ["id", "set", "expected", "danger", "label", "latency_ms", "operation", "risk_tags", "pair", "rule", "cap"]
        )
        writer.writerows(
            [
                case["id"],
                case["set"],
                case["expected"],
                result.danger,
                result.label,
                result.latency_ms,
                case.get("operation") or "",
                "|".join(case.get("risk_tags") or []),
                case.get("pair") or "",
                result.rule or "",
                result.cap or "",
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
                cases.append(
                    {
                        "id": row["id"],
                        "set": row["set"],
                        "expected": row["expected"],
                        "operation": row.get("operation") or None,
                        "risk_tags": [tag for tag in (row.get("risk_tags") or "").split("|") if tag],
                        "pair": row.get("pair") or None,
                    }
                )
                results.append(
                    EvaluationResult(
                        row["id"],
                        float(row["danger"]) if row["danger"] else None,
                        row["label"],
                        float(row["latency_ms"]),
                        row.get("rule") or None,
                        row.get("cap") or None,
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
    if args.by:
        for name, run_cases, results in runs:
            print_grouping(args.by, name, run_cases, results)
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


def fixture_line(cls: type[Evaluator], case: dict[str, Any]) -> dict[str, Any]:
    """What the mode `cls` sends for `case`, and what its rules and caps decide before the
    model answers. `derived.rule` is the rule of the mode that denies the call, and
    `derived.cap` the first tag of its caps that the call has; each is `None` where none does."""
    _, module = contract_of(cls.key)
    request = case["request"]
    workspace = case.get("repository")
    state = state_of(module, request, workspace)
    derived = derive(request, workspace)
    rules = getattr(module, "RULES", [])
    caps = getattr(module, "CAPS", [])
    return {
        "id": case["id"],
        "request": request,
        "workspace": workspace,
        "model": getattr(cls, "model", None),
        "state": state,
        "questions": module.QUESTIONS,
        "derived": {
            "operation": derived.operation_hint,
            "risk_tags": derived.risk_tags,
            "rule": derived.rule if derived.rule in rules else None,
            "cap": next((tag for tag in caps if tag in derived.risk_tags), None),
        },
    }


def cmd_fixture(args: argparse.Namespace) -> int:
    """Print one JSON line per case: the request of a mode and its derived facts. No model."""
    cls = registry.get(args.evaluator)
    cases = cases_mod.load_cases(run_case_files(args.cases, args.heldout))
    if args.real:
        cases.extend(db_mod.load_real_cases())
    for case in cases:
        print(json.dumps(fixture_line(cls, case), ensure_ascii=False, separators=(",", ":")))
    return 0


def contract_of(key: str) -> tuple[type[Evaluator], Any]:
    """The evaluator `key` and its module, which must carry the contract of a mode."""
    cls = registry.get(key)
    module = importlib.import_module(cls.__module__)
    if not hasattr(module, "QUESTIONS") or not (hasattr(module, "state") or hasattr(module, "FIELDS")):
        raise EvaluatorError("%s is not a contract: it has no QUESTIONS, or no state and no FIELDS" % key)
    return cls, module


def state_of(module: Any, request: dict[str, Any], workspace: str | None) -> Any:
    if hasattr(module, "state"):
        return module.state(request, workspace)
    return representations.build_json(request, workspace, module.FIELDS)


def probe_cases(prober: Any, contracts: list[tuple[str, Any]], cases: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """One record per case: the probabilities of each question of each mode at the temperature
    of `prober`, and the derived facts. The modes that build one state go in one request, each
    question under the name `<key>.<question>` of the first mode that asks it: a question that
    two modes ask over one state is asked one time. `prober` gives
    `probabilities(state, questions)`."""
    records = []
    for case in cases:
        request, workspace = case["request"], case.get("repository")
        groups: dict[str, tuple[Any, dict[str, Any], dict[str, list[tuple[str, str]]]]] = {}
        for key, module in contracts:
            state = state_of(module, request, workspace)
            _, questions, askers = groups.setdefault(json.dumps(state, sort_keys=True), (state, {}, {}))
            for name, question in module.QUESTIONS.items():
                text = json.dumps(question, sort_keys=True)
                if text not in askers:
                    questions["%s.%s" % (key, name)] = question
                askers.setdefault(text, []).append((key, name))
        found: dict[str, dict[str, list[float]]] = {key: {} for key, _ in contracts}
        started = time.perf_counter()
        for state, questions, askers in groups.values():
            answered = prober.probabilities(state, questions)
            for asked in askers.values():
                first = "%s.%s" % asked[0]
                for key, name in asked:
                    found[key][name] = answered[first]
        derived = derive(request, workspace)
        records.append(
            {
                "id": case["id"],
                "set": case["set"],
                "expected": case["expected"],
                "operation": case.get("operation"),
                "risk_tags": case.get("risk_tags") or [],
                "pair": case.get("pair"),
                "derived": {
                    "operation_hint": derived.operation_hint,
                    "risk_tags": derived.risk_tags,
                    "rule": derived.rule,
                },
                "latency_ms": (time.perf_counter() - started) * 1000.0,
                "probabilities": found,
            }
        )
    return records


def cmd_probe(args: argparse.Namespace) -> int:
    """Ask the questions of several modes in one model load, at temperature 1.0."""
    from evaluators.kev import KevEvaluator

    contracts = []
    for key in args.evaluator:
        cls, module = contract_of(key)
        if cls.backend != "kev":
            raise EvaluatorError("%s is not a Kev mode" % key)
        if contracts and module.RUN != contracts[0][1].RUN:
            raise EvaluatorError("%s has another run than %s: probe each run alone" % (key, contracts[0][0]))
        contracts.append((key, module))

    class Prober(KevEvaluator):
        key = "probe"
        run = contracts[0][1].RUN
        temperature = 1.0

        def evaluate(self, case: dict[str, Any]) -> Any:
            raise EvaluatorError("a probe evaluates no case")

    cases = cases_mod.load_cases(run_case_files(args.cases, args.heldout))
    if args.real:
        cases.extend(db_mod.load_real_cases())
    prober = Prober()
    prober.setup()
    try:
        records = probe_cases(prober, contracts, cases)
    finally:
        prober.teardown()
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8") as output:
        for record in records:
            output.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
    print("%d records of %s: %s" % (len(records), ", ".join(args.evaluator), out))
    return 0


def read_probe(targets: list[str]) -> list[dict[str, Any]]:
    records = []
    for target in targets:
        try:
            with Path(target).open(encoding="utf-8") as lines:
                records.extend(json.loads(line) for line in lines if line.strip())
        except (OSError, json.JSONDecodeError) as exc:
            raise EvaluatorError("could not read the probe records %s: %s" % (target, exc)) from exc
    if not records:
        raise EvaluatorError("no probe records in %s" % ", ".join(targets))
    return records


def print_rows(headers: list[str], rows: list[list[str]]) -> None:
    widths = [len(header) for header in headers]
    for row in rows:
        widths = [max(width, len(value)) for width, value in zip(widths, row)]
    print("  ".join(header.ljust(width) for header, width in zip(headers, widths)))
    print("  ".join("-" * width for width in widths))
    for row in rows:
        print("  ".join(value.ljust(width) for value, width in zip(row, widths)))


def share(count: int, total: int) -> str:
    return "%d of %d (%s)" % (count, total, "%.3f" % (count / total) if total else "-")


OUTCOME_HEADERS = [
    "allow threshold",
    "deny threshold",
    "pair",
    "allow margin",
    "deny margin",
    "benign allowed",
    "adversarial denied",
    "risky allowed",
    "benign denied",
    "AUROC risky",
    "AUROC deny",
]


def outcome_row(found: dict[str, Any]) -> list[str]:
    return [
        "%.4f" % found["allow_threshold"],
        "%.4f" % found["deny_threshold"],
        "yes" if found["has_pair"] else "no",
        format_value(found["allow_margin"]),
        format_value(found["deny_margin"]),
        share(found["benign_allowed"], found["benign"]),
        share(found["adversarial_denied"], found["adversarial"]),
        str(found["risky_allowed"]),
        str(found["safe_denied"]),
        format_value(found["auroc_risky"]),
        format_value(found["auroc_deny"]),
    ]


def cmd_measure(args: argparse.Namespace) -> int:
    """Print one variable of a mode from probe records, with no model."""
    records = read_probe(args.targets)
    _, module = contract_of(args.evaluator)
    if any(args.evaluator not in record["probabilities"] for record in records):
        raise EvaluatorError("the probe records do not carry %s" % args.evaluator)
    caps = list(getattr(module, "CAPS", [])) if args.caps is None else [tag for tag in args.caps.split(",") if tag]
    rules = list(getattr(module, "RULES", [])) if args.rules is None else [rule for rule in args.rules.split(",") if rule]
    for name, known in (("cap", TAGS), ("rule", RULES)):
        for value in caps if name == "cap" else rules:
            if value not in known:
                raise EvaluatorError("unknown %s %r" % (name, value))
    temperatures = args.temperature or [module.TEMPERATURE]
    if any(temperature is None for temperature in temperatures):
        raise EvaluatorError("%s keeps the temperature of its checkpoint: give --temperature" % args.evaluator)
    print("%s, caps: %s, rules: %s, margin %.2f" % (args.evaluator, ", ".join(caps) or "-", ", ".join(rules) or "-", args.margin))
    if args.variable == "temperature":
        rows = []
        for temperature in temperatures:
            found = probe.results(records, args.evaluator, module.QUESTIONS, module.danger, temperature, caps, rules)
            if args.pair:
                outcome = probe.at_pair(records, found, args.pair[0], args.pair[1])
            else:
                outcome = probe.outcome(records, found, args.margin)
            rows.append(["%.2f" % temperature, *outcome_row(outcome)])
        print_rows(["temperature", *OUTCOME_HEADERS], rows)
    elif args.variable == "caps":
        rows = probe.cap_costs(records, args.evaluator, module.QUESTIONS, module.danger, temperatures[0], caps, rules, args.margin)
        print_rows(
            ["tag", "benign", "risky", "cost", "allow threshold", "benign allowed", "gain"],
            [
                [
                    row["tag"],
                    str(row["benign"]),
                    str(row["risky"]),
                    str(row["cost"]),
                    "%.4f" % row["allow_threshold"],
                    str(row["benign_allowed"]),
                    "%+d" % row["gain"],
                ]
                for row in rows
            ],
        )
    elif args.variable == "policy":
        found = probe.probability_policy(records, args.evaluator, args.question, temperatures[0], caps, rules, args.margin)
        print_rows(
            ["allow probability", "deny probability", "benign allowed", "adversarial denied", "risky allowed", "benign denied"],
            [
                [
                    "%.4f" % found["allow_probability"],
                    "%.4f" % found["deny_probability"],
                    share(found["benign_allowed"], found["benign"]),
                    share(found["adversarial_denied"], found["adversarial"]),
                    str(found["risky_allowed"]),
                    str(found["safe_denied"]),
                ]
            ],
        )
    else:
        options = list(module.QUESTIONS[args.question]["criteria"])
        found = probe.choice_accuracy(records, args.evaluator, args.question, options)
        print_rows(
            ["operation", "correct"],
            [
                *([operation, share(hits, total)] for operation, (hits, total) in found["by_operation"].items()),
                ["every operation", share(round((found["accuracy"] or 0) * found["count"]), found["count"])],
            ],
        )
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
    if args.by:
        for name, run_cases, results in runs:
            print_grouping(args.by, name, run_cases, results)
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
    run.add_argument("--by", choices=("operation", "tag", "pair"), help="print a breakdown table per operation, risk tag or adversarial pair")
    run.set_defaults(func=cmd_run)
    derived = subcommands.add_parser("derive", help="report deterministic risk tags and hard rules")
    derived.add_argument("--cases", nargs="+", help="case files or directories (default: the development cases); a directory never adds its held-out files")
    derived.add_argument("--heldout", action="store_true", help="use the held-out cases, or add them after --cases")
    derived.add_argument("--real", action="store_true", help="also load approved cases from ~/.ariadne/ariadne.db read-only")
    derived.set_defaults(func=cmd_derive)
    fixture = subcommands.add_parser("fixture", help="print what a mode sends and derives for each case, with no model")
    fixture.add_argument("--evaluator", required=True, help="an evaluator key from `list`")
    fixture.add_argument("--cases", nargs="+", help="case files or directories (default: the development cases); a directory never adds its held-out files")
    fixture.add_argument("--heldout", action="store_true", help="use the held-out cases, or add them after --cases")
    fixture.add_argument("--real", action="store_true", help="also load approved cases from ~/.ariadne/ariadne.db read-only")
    fixture.set_defaults(func=cmd_fixture)
    probing = subcommands.add_parser("probe", help="record the probabilities of several Kev modes in one model load, at temperature 1.0")
    probing.add_argument("--evaluator", action="append", required=True, help="a Kev evaluator key from `list`; repeat for each mode")
    probing.add_argument("--cases", nargs="+", help="case files or directories (default: the development cases); a directory never adds its held-out files")
    probing.add_argument("--heldout", action="store_true", help="use the held-out cases, or add them after --cases")
    probing.add_argument("--real", action="store_true", help="also load approved cases from ~/.ariadne/ariadne.db read-only")
    probing.add_argument("--out", required=True, help="the JSON Lines file for the records")
    probing.set_defaults(func=cmd_probe)
    measure = subcommands.add_parser("measure", help="print one variable of a mode from probe records, with no model")
    measure.add_argument("variable", choices=("temperature", "caps", "policy", "operation"))
    measure.add_argument("targets", nargs="+", help="files of probe records; their records go together")
    measure.add_argument("--evaluator", required=True, help="the mode to measure, by key")
    measure.add_argument("--temperature", type=float, action="append", help="a temperature; repeat for each one (default: the one of the mode)")
    measure.add_argument("--caps", help="tags that cap, with commas (default: the CAPS of the mode; '' for none)")
    measure.add_argument("--rules", help="hard rules in use, with commas (default: the RULES of the mode; '' for none)")
    measure.add_argument("--margin", type=float, default=DEFAULT_MARGIN, help="selection margin (default: %s)" % DEFAULT_MARGIN)
    measure.add_argument("--pair", type=float, nargs=2, metavar=("ALLOW", "DENY"), help="temperature only: judge at this pair, not at the pair of the records")
    measure.add_argument("--question", default="decision", help="policy and operation: the name of the choice question (default: decision)")
    measure.set_defaults(func=cmd_measure)
    report = subcommands.add_parser("report", help="print one table from per-case CSV files that earlier runs wrote")
    report.add_argument("targets", nargs="+", help="CSV files or directories of them")
    report.add_argument("--by", choices=("operation", "tag", "pair"), help="print a breakdown table per operation, risk tag or adversarial pair")
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
