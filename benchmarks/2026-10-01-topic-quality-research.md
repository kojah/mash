# Research directions for topic quality

This is a research and experiment plan, not evidence that any new method has
passed Mash's relevance gates. Topic modeling is now the first priority. Follow
the updated sequence: compile research, shortlist approaches, experiment in
Python and record results, then implement the selected approach in Rust.
Track the research lab in `mash-c0d.19.2.21` and quality work in
`mash-c0d.19.2.12`. The Rust
topic implementation remains an offline prototype; production topic MCP
integration must still pass quality and lifecycle checks.

## Label representation

[BERTopic (Grootendorst, 2022)](https://arxiv.org/abs/2203.05794) separates
embedding/clustering from class-based TF-IDF topic representation. This supports
evaluating labels independently of BIRCH discovery. Mash already has a c-TF-IDF
baseline; its current support-aware experiment still admits generic labels and
can lose concrete secondary subjects.

The author's [representation documentation](https://maartengr.github.io/BERTopic/getting_started/representation/representation.html)
describes two useful refinements:

- **KeyBERTInspired:** select representative documents using c-TF-IDF, build a
  topic embedding from them, and compare candidate phrases using the same
  embedding model. Our proposed adaptation is a bounded selection of supporting
  segments. Compare this label-scoring representation with the existing cluster
  centroid without changing discovery, assignments or source evidence.
- **MMR:** balance candidate relevance with similarity to labels already
  selected. Compare this with our current hard redundancy rules. Retain concrete
  phrases and minority subjects; greater diversity alone is not success.

The author's [c-TF-IDF documentation](https://maartengr.github.io/BERTopic/getting_started/ctfidf/ctfidf.html)
also describes class-based BM25 and square-root normalized term-frequency
weighting. Evaluate these as separate ablations. They are not a justification
for blindly deleting frequent domain words or rare technical identifiers.

Evaluate these algorithms in the isolated [Python lab](../research/topics/README.md)
first. Any selected production implementation will remain Rust; the research
environment does not become an application dependency.

## Conversation-specific prior art and experiment order

[ChatGPT Choreography](https://doi.org/10.1109/MC.2025.3532347) applies topic
modeling and thematic analysis to developer–ChatGPT dialogues. Its publisher
abstract supports the application precedent; it does not establish parameters
or quality for coding-agent traces. [DevGPT](https://github.com/NAIST-SE/DevGPT)
provides an external developer-conversation corpus, with shared-conversation
selection bias and different tool-trace coverage from our local histories.

[Contextual Topic Modeling for Dialog Systems](https://arxiv.org/abs/1810.08135)
uses conversational context and dialogue acts for topic classification. This
motivates context ablations, rather than evidence that every short reply should
borrow the following assistant answer. Native source grounding establishes
provenance, not whether the borrowed text introduces a different subject.

[Xing and Carenini](https://aclanthology.org/2021.sigdial-1.18/) learn utterance-pair
coherence for dialogue segmentation. [Gao et al.](https://arxiv.org/abs/2305.02747)
learn topic-aware utterance representations. Both motivate testing episodes
before clustering long sessions; neither proves a raw adjacent-cosine threshold
is enough. Compare bounded windows, turn pairs and lexical boundary evidence
before adding trained segmentation assets. Online episodes need provisional
boundaries and explicit rules for late replies, edits and deletions.

[MemTree](https://arxiv.org/abs/2410.14052) incrementally organizes memories and
aggregates parent representations with LLMs. It is useful hierarchy-maintenance
prior art, but its QA evaluation does not validate granular multi-label topic
quality, and its generative cost differs from our intended default. Neither a
memory tree nor BIRCH's physical CF tree automatically constitutes a semantic
taxonomy.

Experiment order: frozen raw geometry controls; independently annotated dialogue
units and referential-context selection; label-only contrastive c-TF-IDF and
representative-phrase scoring; then online discovery and semantic hierarchy.
Hold units and labels fixed when comparing algorithms. Avoid changing cleanup,
segmentation, clustering and labels together. Existing filler-removal retrieval
pilots did not demonstrate a gain; keep production embedding inputs unchanged.

## Evaluation

[Reading Tea Leaves (Chang et al., 2009)](https://papers.nips.cc/paper_files/paper/2009/file/f92586a25bb3145facd64ab20fd554ff-Paper.pdf)
introduces word intrusion to test label coherence and topic intrusion to test
document/topic agreement. Adapt both to thread content and pair them with
independent major/secondary subject annotations. A coherent label set can still
be attached to the wrong thread.

[Röder, Both and Hinneburg (2015)](https://doi.org/10.1145/2684822.2685324)
compare coherence measures and introduce C_v, combining sliding-window
co-occurrence, NPMI and indirect cosine comparison. NPMI/C_v can provide useful
offline diagnostics, with reference corpus, tokenization, window size and
sampling recorded. They do not directly measure assignment precision, secondary
topic recall or ancestor quality.

[Hoyle et al. (2021)](https://arxiv.org/abs/2107.02173) find that automatic
coherence can declare model differences unsupported by human evaluation. Do not
automatically delete topics based on a coherence cutoff. Compare held-out,
insertion-order and provider results and review full source content, not just
the most favorable excerpts. Keep an explicit unassigned state when assignment
evidence is insufficient.

## Assessing the proposed denoising checklist

The four boundaries—source admission, geometry, assignment and representation—
are useful places to inspect failures. The proposed numeric thresholds are
unvalidated for our data and should not become defaults.

- **Mean centering / principal-component removal:**
  [All-but-the-Top (Mu et al.)](https://arxiv.org/abs/1702.01417) provides research
  support for postprocessing word representations;
  [sentence whitening (Su et al., 2021)](https://arxiv.org/abs/2103.15316) studies
  sentence representations. These results motivate an ablation, not a guarantee
  for our trained sentence model. Freeze and version the transform. Updating the
  mean/PCA basis while inserting new points would otherwise mix coordinates in
  an existing BIRCH tree unless old statistics are transformed or rebuilt too.
  Apply any selected transform consistently to document and label vectors.
- **Near-origin noise:** unit-normalized nonzero vectors all have unit norm.
  Distance from the origin cannot distinguish noise in that space. Invalid/zero
  vector validation is a separate issue from semantic quality.
- **k-NN percentile rejection:** removing a fixed 5–10% risks deleting legitimate
  rare subjects. Start with diagnostic flags and annotated rare-topic examples,
  not automatic source deletion.
- **HDBSCAN:** a useful offline comparator, but it does not replace streaming
  discovery by itself. The supplied `min_samples` guidance is reversed:
  [official documentation](https://hdbscan.readthedocs.io/en/latest/parameter_selection.html)
  says larger values are more conservative and label more points as noise.
- **Cosine rejection:** sensible as a calibrated assignment boundary. Measure
  precision/recall by segment and thread; do not assume 0.45–0.55 transfers
  between models, transforms or topics.
- **Frequency and minimum-length pruning:** short queries such as “Redis HNSW”
  and rare identifiers can carry useful subjects. Prefer source-aware framing
  and label-only conversational masking to blanket token-count/frequency rules.
- **High-entropy soft memberships:** multiple subjects are intentional in Mash.
  Ambiguity and genuine topic overlap need separate evidence. Softmax scores are
  not calibrated probabilities without validation, and shared ancestor tags
  must not be treated as independent competing topics.
- **Random-pair / coherence screening:** useful flags, not deletion rules. Define
  the reference distribution and check sample size, broad parent topics and
  rare subjects before interpreting a cutoff.

Prioritize representative-segment label scoring and MMR after the conversational
masking experiment. Evaluate geometry changes separately so a label improvement
cannot conceal lost thread assignments. Preserve the original search index and
source history throughout these topic experiments.

## Granular domain subjects

The second proposed checklist adds useful representation experiments, but its
claim that leading principal components always encode generic syntax/sentiment
is not a diagnosis of our model. Measure the failure first. Topic labels alone
cannot establish whether a cluster is geometrically bad or merely badly named.

[Tomokiyo and Hurst (2003)](https://aclanthology.org/W03-1805/)
score both phrase cohesion and informativeness using foreground/background
language models. This motivates a bounded contrastive phrase-scoring ablation:
reward phrases supported by this topic while penalizing conversational
background shared across topics. Start with a versioned background derived from
training-partition data, with smoothing and independent-thread support. Compare
against an external reference only separately; general-English corpora have
different provider/code/technical vocabulary distributions. Neither background
is automatically a trustworthy stopword list.

Technical compounds and entities are useful candidates. Compare noun-phrase
patterns and code-aware identifiers with the current bounded n-grams; do not
force a closed domain vocabulary or remove every verb/adjective. Mash's history
spans software, research and ordinary planning, and new subjects must remain
discoverable. POS filtering can be a proposal mechanism, not a hard ban on
“deadlock,” “Go,” API paths or unfamiliar names.

For MMR, retain the topic as the relevance target and selected phrases as the
redundancy comparison. Replacing that target with the global domain centroid
would favor domain-wide similarity. If a global background embedding helps,
evaluate its penalty as a separate, explicitly recorded contrastive term.

[TSDAE (Wang, Reimers and Gurevych, 2021)](https://aclanthology.org/2021.findings-emnlp.59/)
and [Sentence Transformers' domain-adaptation guidance](https://www.sbert.net/examples/sentence_transformer/domain_adaptation/README.html)
provide a basis for later domain adaptation. Pretraining objectives and retrieval
objectives are not interchangeable. Do not assume MLM alone produces better
sentence similarities or that TSDAE is cheap enough for our iteration budget.
Fine-tuning requires model versioning, corpus-wide re-embedding and independent
retrieval/topic checks. Canonical source-group splits and duplicate/false-negative
handling matter for contrastive training on copied conversations.

Dense-plus-lexical evidence is also worth testing for technical subjects. BM25
is a query-dependent score, not a dense vector to append directly. Define the
sparse representation, normalization, weighting and streaming work bound before
combining spaces, or compare lexical and semantic evidence at the scoring
boundary instead. Seed guidance should remain optional; known domains cannot
become a prerequisite for discovering new ones.

Coarse-to-fine labeling may help distinguish broad ancestors from specific leaf
subjects, but generic clusters must not be deleted wholesale: a mixed thread can
contain both conversational planning and a valuable technical subject. Neither
BIRCH's CF topology nor HDBSCAN's density hierarchy automatically supplies a
meaningful topic taxonomy. Test parent/child specificity and ancestor inheritance
against sources independently.
