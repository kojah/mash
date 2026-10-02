# Blind review validation and categorical agreement

Follow-up to the blind pilot, tracked by `mash-c0d.19.2.33.1.1`.
`research/topics/topic_review.py` accepts complete declared-human submissions
only. It checks the exact packet-byte hash, schema keys, complete unique source
and case identities, explicit literal declarations, primary/secondary subjects,
context decisions and exact source quotes. Pair evidence must cover both
endpoints. Blank templates are rejected; null is not an uncertainty judgment.
Nonpositive common-subject fields must be null. Insufficient context requires
uncertain pair judgments. JSON duplicate keys and nonfinite constants are rejected.

With two submissions and the separate selection manifest, the tool reports
three-category confusion matrices, observed agreement, Cohen's kappa and counts
with either reviewer uncertain, separately for the primary and each diagnostic
stratum. Missing strata have undefined agreement; constant-category agreement
has undefined kappa. Uncertainty is retained as a category, not dropped.
Distinct declared reviewer identities are required, but declarations cannot
prove independent reviewers or genuine human authorship. Exposure declarations
are preserved in the report. No accuracy, topic-name agreement, semantic
ancestor correctness or statistical confidence intervals are inferred. Shared
endpoints remain dependent; enriched diagnostics cannot estimate prevalence.

Commands:

```sh
python research/topics/topic_review.py --packet review.json --answers completed.json
python research/topics/topic_review.py --packet review.json --answers completed.json \
  --second other.json --manifest unblinded.json
```

Nine synthetic tests exercise rejection of unanswered/altered/duplicate/missing
answers and invented/wrong-scope evidence, Unicode quotes, context/decision
consistency, literal types, stratum separation, uncertainty, empty/degenerate
statistics, nonconstant perfect agreement and duplicate JSON/nonfinite rejection.
These are software fixtures, not fabricated human annotations. The original
pilot packet and empty answers remain unchanged. No human reviews have arrived;
quality, independent agreement and guided-search utility remain unmeasured.
