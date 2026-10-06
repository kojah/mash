"""Aggregate-only Markdown and LaTeX report for the registered encoder study."""

from pathlib import Path

import click
import orjson

from corpus import Record
from run_voyage_calibration import Outcome, Split
from voyage_vectors import Model, Receipt

ARMS = (
    "minilm",
    "code-float",
    "code-int8",
    "code-complete",
    "general-float",
    "general-int8",
    "general-complete",
)


class Audit(Record):
    targeted_inputs: int
    old_code_codec_max_abs: float
    old_code_codec_mean_cosine: float
    checked_histories: int


class RepeatProbe(Record):
    repeat_raw_cosines: tuple[float, ...]
    repeat_raw_max_absolute: float
    prior_code_codec_cosines: tuple[float, ...]
    repeat_probe_cost: float


def load(root: Path, arm: str, suffix: str) -> Outcome:
    return Outcome.model_validate_json((root / f"{arm}-{suffix}.json").read_bytes())


@click.command()
@click.option("--root", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--markdown", type=click.Path(path_type=Path), required=True)
@click.option("--latex", type=click.Path(path_type=Path), required=True)
def main(root: Path, markdown: Path, latex: Path) -> None:
    split = Split.model_validate_json((root / "split.json").read_bytes())
    audit = Audit.model_validate_json((root / "audit.json").read_bytes())
    receipts = tuple(
        Receipt.model_validate_json(line)
        for line in (root / "receipts.jsonl").read_bytes().splitlines()
    )
    selected = tuple(load(root, arm, "selected") for arm in ARMS)
    header = [
        "# Fresh MiniLM / Voyage 4 / Voyage Code 4 comparison",
        "",
        "Production and the frozen complete-answer-v1 baseline remain unchanged.",
        "",
        f"Matched cohort: {len(split.calibration) + len(split.evaluation)} threads; "
        f"{len(split.calibration)} calibration / {len(split.evaluation)} evaluation. "
        f"{len(split.excluded)} histories excluded by credential screening.",
        "",
        "This is a thread-separated holdout within previously examined development "
        "data, with provisional agent-authored labels. "
        "Original confirmation stays sealed. "
        "Do not treat this as independent final validation.",
        "",
        "Each arm received the same 48 Optuna trials. Parameters and view "
        "were selected using calibration F1 only. Evaluation results "
        "did not select or retune models.",
        "",
        "## Implementation audit",
        "",
        "Response indices/model/dimensions, finiteness, unit norms, "
        "frozen source hashes, "
        "point ownership and complete-source splitting were validated. Reconstructed "
        "MiniLM fixed predictions match the frozen baseline exactly.",
        "",
        f"Original targeted-pooling inputs: {audit.targeted_inputs}. Prior Code 4 "
        f"codec parity checked {audit.checked_histories} histories: maximum coordinate "
        f"difference {audit.old_code_codec_max_abs:.8g}; mean corresponding-vector "
        f"cosine {audit.old_code_codec_mean_cosine:.10f}. "
        "The first remote cache discarded "
        "raw floats; this study retains them before any normalization/quantization.",
        "",
        "## Evaluation after calibration",
        "",
        "| Representation | Precision | Recall | F1 | Long F1 | Distant recall "
        "| F1 gain vs frozen control (95% CI) |",
        "|---|---:|---:|---:|---:|---:|---|",
    ]
    table = []
    for result in selected:
        value = result.overall
        interval = result.interval
        control = load(root, "minilm", "fixed-union").overall.f1
        header.append(
            f"| {result.arm.removesuffix('-selected')} | {value.precision:.2%} | "
            f"{value.recall:.2%} | {value.f1:.2%} | {result.long.f1:.2%} | "
            f"{result.distant_recall:.2%} | {100 * (value.f1 - control):+.2f} pp "
            f"[{100 * interval.gain_low:+.2f}, {100 * interval.gain_high:+.2f}] |"
        )
        table.append(
            result.arm.removesuffix("-selected").replace("-", " ")
            + " & "
            + " & ".join(
                f"{100 * v:.2f}"
                for v in (
                    value.precision,
                    value.recall,
                    value.f1,
                    result.long.f1,
                    result.distant_recall,
                )
            )
            + r" \\"
        )
    header += [
        "",
        "Intervals use 2,000 paired thread bootstrap resamples; they are "
        "exploratory and unadjusted for multiple arms.",
        "",
        "## Chosen parameters",
        "",
        "| Arm | Radius | Cutoff | Relative margin | View |",
        "|---|---:|---:|---:|---|",
    ]
    for result in selected:
        p = result.parameters
        header.append(
            f"| {result.arm} | {p.radius:.2f} | {p.cutoff:.2f} "
            f"| {p.margin:.2f} | {p.view} |"
        )
    header += [
        "",
        "## Fixed-setting view and codec ablations",
        "",
        "Radius 0.60, cutoff 0.75, relative margin 0.05; evaluation threads only.",
        "",
        "| Arm | Pooled F1 | Chunk F1 | Union F1 | Union precision | Union recall |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    for arm in ARMS:
        a, b, both = (
            load(root, arm, "fixed-" + view).overall
            for view in ("pooled", "chunk", "union")
        )
        header.append(
            f"| {arm} | {a.f1:.2%} | {b.f1:.2%} | {both.f1:.2%} | "
            f"{both.precision:.2%} | {both.recall:.2%} |"
        )
    header += [
        "",
        "## Calibrated model comparisons",
        "",
        "| Arm | F1 gain vs calibrated MiniLM | 95% paired interval |",
        "|---|---:|---|",
    ]
    for arm in ARMS[1:]:
        comparison = load(root, arm, "vs-calibrated-minilm")
        ci = comparison.interval
        gain = 100 * (comparison.overall.f1 - selected[0].overall.f1)
        header.append(
            f"| {arm} | {gain:+.2f} pp | "
            f"[{100 * ci.gain_low:+.2f}, {100 * ci.gain_high:+.2f}] |"
        )
    header += [
        "",
        "## API usage",
        "",
        "| Model | Billed tokens | Recorded cost |",
        "|---|---:|---:|",
    ]
    for model in Model:
        matching = tuple(r for r in receipts if r.model == model)
        header.append(
            f"| {model.value} | {sum(r.tokens for r in matching):,} | "
            f"${sum(r.cost for r in matching):.6f} |"
        )
    header += [
        "",
        "Raw embeddings, receipts, per-thread predictions, Optuna and MLflow "
        "provenance remain in private research storage. All variants can now be "
        "rescored without additional embedding charges.",
        "",
        "Complete-turn inputs join current user prompts and all visible assistant "
        "segments with role markers. They use a lossless conservative 28,000-byte "
        "structural budget, normalized mean pooling only for oversized turns. "
        "This changes both input granularity and external role weighting; it does "
        "not isolate context length alone. No LLM summaries are used.",
    ]
    probe_path = root / "repeat-probe.json"
    if probe_path.exists():
        probe = RepeatProbe.model_validate_json(probe_path.read_bytes())
        header += [
            "",
            "## Repeat-call stability probe",
            "",
            "A diagnostic three-input Code 4 repeat call, using differently batched "
            "inputs, produced corresponding raw-vector cosines "
            f"{min(probe.repeat_raw_cosines):.8f}–"
            f"{max(probe.repeat_raw_cosines):.8f}, with maximum coordinate "
            f"difference {probe.repeat_raw_max_absolute:.8f}. Its "
            f"${probe.repeat_probe_cost:.8f} charge is included above. "
            "This does not determine whether variation comes from batching, "
            "server numerics or other provider behavior. It is not a full "
            "clustering stability study. The probe overlapped the general-model "
            "build, briefly adding one request beyond the two-request builder "
            "limit; no quality trial or configuration was added. "
            "Float/int8 comparisons use the same fresh raw vectors, avoiding "
            "this repeat-call confound.",
        ]
    correction = root / "trial-count-correction.md"
    if correction.exists():
        header += ["", "## Harness correction", "", correction.read_text().strip()]
    header += [
        "",
        "## Interpretation",
        "",
        "Model-specific calibration matters: Code 4 float union improves from "
        f"{load(root, 'code-float', 'fixed-union').overall.f1:.2%} at the local "
        f"settings to {load(root, 'code-float', 'selected').overall.f1:.2%}; "
        "Voyage 4 float improves from "
        f"{load(root, 'general-float', 'fixed-union').overall.f1:.2%} to "
        f"{load(root, 'general-float', 'selected').overall.f1:.2%}. "
        "The local frozen configuration remains its calibration winner.",
        "",
        "Float versus int8 changes little at fixed settings. Code 4 fixed union "
        "has identical aggregate precision/recall/F1 in both precisions; Voyage 4 "
        "differs slightly. This does not implicate quantization as the main cause "
        "of the original regression.",
        "",
        "View quality is model-dependent: Voyage 4 chunk-only outperforms the "
        "other chunk-only arms at the fixed settings, whereas Code 4 chunk-only "
        "is weaker. A naive union can increase false positives: fixed Voyage 4 "
        "union has high recall but low precision. Calibration narrows its radius "
        "from 0.60 to 0.50 and restores a more useful balance.",
        "",
        "Complete-turn Code 4 is weaker than its calibrated short-window version; "
        "complete-turn Voyage 4 trades some recall for precision with a similar "
        "overall F1. Direct whole-turn embedding is not a universal improvement.",
        "",
        "Voyage 4 is promising for distant-turn recall, but all remote F1 gain "
        "intervals include zero. The frozen local baseline stays unchanged. "
        "These observations do not establish model equivalence or a statistically "
        "clear winner. Bootstrap intervals capture sampled-thread uncertainty, "
        "not provider inference variability or annotation bias.",
    ]
    markdown.write_text("\n".join(header) + "\n")
    latex.write_text(
        r"""\documentclass[11pt]{article}
\usepackage[margin=1in]{geometry}
\usepackage{booktabs}
\usepackage[hidelinks]{hyperref}
\title{Turn clustering: a calibrated encoder comparison}
\author{Mash experimental research}
\date{6 October 2026}
\begin{document}
\maketitle
\section{Procedure}
We compare frozen all-MiniLM-L6-v2 representations with Voyage 4 and Voyage
Code 4. The matched cohort contains 156 development histories after four
credential-screening exclusions. A deterministic SHA-256 ordering assigns
104 histories to calibration and 52 to evaluation. Previously inspected
development labels are provisional agent judgments, not a pristine final test;
the original confirmation remains sealed. Each representation receives the
same 48 random-sampler Optuna trials and parameter draws, optimizing only
calibration F1. BIRCH radius, absolute membership cutoff, relative-best margin
and view are calibrated. Fitting and smoothing remain within each thread.

The local baseline uses quantized MiniLM and equal-role pooling of short
structural windows. Each remote model is tested using normalized raw floats,
the original signed-int8 codec, and complete-turn floats. Complete-turn inputs
include all current prompts and visible assistant segments with role markers.
They are split losslessly at paragraph/line boundaries if exceeding a
conservative 28,000 UTF-8-byte budget; oversize pieces have a normalized-mean
pooled view. This intervention also changes role weighting and is not a pure
context-length ablation. No LLM summaries or other-thread context are used.
Provider-default task mode, 1024 dimensions, denied data collection and disabled
fallbacks are held fixed. Raw floats and model-specific exact-text cache keys
are retained. The first remote experiment cached only int8-decoded vectors.

\section{Evaluation results}
All entries are percentages on the 52 evaluation histories. Long histories
have at least eight turns; distant pairs have turn gap at least four within
long histories. Models/configurations are not selected using these results.
\begin{center}\small
\begin{tabular}{lrrrrr}
\toprule
Representation & Precision & Recall & F1 & Long F1 & Distant recall\\
\midrule
"""
        + "\n".join(table)
        + r"""
\bottomrule
\end{tabular}
\end{center}
The companion Markdown report records fixed-view and codec ablations, selected
parameters, API charges and paired 2,000-resample thread-bootstrap intervals.
Intervals are exploratory and unadjusted for multiple comparisons. Validation
checks response ordering, dimensions, norms, ownership, source preservation and
frozen-control predictions. MLflow stores parameters, hashes and aggregate
metrics only; raw histories and detailed labels remain private.

\section{Analysis}
Calibration substantially improves the remote float pipelines relative to
using the local model's settings. Float/int8 fixed-setting differences are
small, so quantization is not supported as the main explanation for the
original regression. Chunk quality differs by model: the general-purpose
Voyage 4 chunk view is stronger than Code 4's here. A naive union can trade
precision away for recall; calibrating the BIRCH radius improves that balance.
Complete-turn input is not a universal improvement: it loses quality for
Code 4 and trades recall for precision for Voyage 4. All remote F1-gain
bootstrap intervals include zero. Voyage 4's distant-recall point gains are
promising but provisional; the frozen local baseline remains unchanged.

An off-by-one harness error counted the initially queued baseline trial as an
execution. Detected after the first local evaluation and before remote-arm
evaluation, it was corrected by restarting calibration from scratch on the
same split/cache; all final arms complete 48 trials. Original records and
source are archived. A three-input diagnostic repeat-call probe found small
Code 4 float-vector changes across differently batched calls; their cause is
not established. Float/int8 arms share the same raw vectors. No quality trial
was added by that probe. Provider repeat variability is not included in the
thread-bootstrap uncertainty estimates.

\section{Sources and limitations}
The frozen study remains a reference and production behavior is unchanged.
Remote model weights cannot be pinned, and repeated API calls may expose
provider drift. Calibration/holdout division cannot undo earlier inspection
of this development corpus. Human-reviewed, independently held-out evaluation
is still required before strong quality claims.
\begin{thebibliography}{9}
\bibitem{minilm} Sentence Transformers. all-MiniLM-L6-v2 model card.
\url{https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2}
\bibitem{voyage} Voyage AI. Text embeddings: models and API task modes.
\url{https://docs.voyageai.com/docs/embeddings}
\bibitem{code} Voyage AI. Voyage-code-4: code retrieval built for coding agents.
\url{https://blog.voyageai.com/2026/08/13/voyage-code-4/}
\bibitem{optuna} T. Akiba et al. Optuna: A Next-generation Hyperparameter
Optimization Framework. KDD, 2019. \url{https://arxiv.org/abs/1907.10902}
\bibitem{mlflow} MLflow. Tracking APIs.
\url{https://mlflow.org/docs/latest/ml/tracking/tracking-api/}
\end{thebibliography}
\end{document}
"""
    )
    click.echo(
        orjson.dumps(
            {"api_cost": sum(r.cost for r in receipts), "arms": len(selected)}
        ).decode()
    )


if __name__ == "__main__":
    main()
