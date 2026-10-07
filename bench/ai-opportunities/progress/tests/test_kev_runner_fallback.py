"""A missing interpreter, or one whose `kev` is not pinned to the required revision, is reported
as an infrastructure blocker, not silently skipped or treated as a model-quality result (common
acceptance criterion)."""
import pytest
from bench_progress.kev_runner import check_kev_revision, run_kev_batch


def test_a_missing_kev_interpreter_raises_a_clear_blocker():
    with pytest.raises(RuntimeError, match="no kev interpreter"):
        run_kev_batch(
            prompt_path="prompts/v2.json",
            input_path="cases/eval.jsonl",
            output_path="/tmp/does-not-matter.jsonl",
            stats_path="/tmp/does-not-matter-stats.json",
            policy_name="kev",
            kev_python="/no/such/interpreter",
        )


def test_checking_the_revision_of_a_missing_interpreter_raises_a_clear_blocker():
    with pytest.raises(RuntimeError, match="no interpreter"):
        check_kev_revision(kev_python="/no/such/interpreter")
