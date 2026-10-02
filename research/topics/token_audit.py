"""Pinned public source-only audit of technical token boundaries and eligibility."""
import argparse
import json
from pathlib import Path
import time
import unicodedata

from external_context import compare
from question_channels import Channel, classify
from reference_review import digest
from stackoverflow_source import load
from token_boundaries import audit, summarize
from weighted_questions import CHANNEL_PIN, GRAPH_PIN, capture


def experiment(root):
    started = time.perf_counter()
    channels = capture(root, 'channels-first.json', CHANNEL_PIN)
    graph = capture(root, 'graph-first.json', GRAPH_PIN)
    topic_raw = (root / 'topics-first.json').read_bytes()
    if digest(topic_raw) != channels['policy']['original_topic_sha256']:
        raise ValueError('original feature capture required')
    vocabulary = json.loads(topic_raw)['vocabulary']
    original = {r['id']: r for r in channels['records']}
    records = []
    for source in load(root).inputs:
        identity = str(source.id)
        prior = original[identity]
        if prior['status'] != 'supported' or prior['source_sha256'] != source.source_sha256:
            raise ValueError('complete previously audited source required')
        rendered = classify(source.body_html)
        text = rendered.rendered.text
        if digest(text.encode()) != prior['rendered_sha256']:
            raise ValueError('original displayed source changed')
        title_spans = audit(source.title, vocabulary=vocabulary, graph_words=graph['words'])
        records.append({'id': identity, 'field': 'title', 'channel': 'title', 'text_sha256': digest(source.title.encode()),
                        'spans': title_spans})
        body_matches = []
        for run in rendered.runs:
            if run.channel not in (Channel.PROSE, Channel.CODE):
                continue
            spans = audit(text[run.start:run.end], run.start, run.byte_start, vocabulary, graph['words'])
            body_matches.extend((m['start'], m['end'], m['term'], run.channel) for span in spans for m in span['matches'])
            records.append({'id': identity, 'field': 'body_displayed', 'channel': run.channel,
                            'text_sha256': digest(text.encode()), 'run_start': run.start, 'run_end': run.end,
                            'spans': spans})
        if body_matches != [(t['start'], t['end'], t['term'], t['channel']) for t in prior['tokens']]:
            raise ValueError('complete original body-token inventory required')
    return {'version': 1, 'scope': 'all200-source-token-boundaries-no-topic-outcomes',
            'policy': {'channel_sha256': CHANNEL_PIN, 'graph_sha256': GRAPH_PIN,
                       'maximal_runs': 'Unicode-alphanumeric-underscore-marks-plus-dot-plus-colon-slash-hash-hyphen',
                       'normalization': 'original-lower-not-casefold', 'original_vocabulary_size': len(vocabulary),
                       'graph_vocabulary_size': len(graph['words']), 'semantic_tokenizer_selected': False,
                       'embedding_inference_calls': 0, 'unicode_version': unicodedata.unidata_version},
            'records': records, 'summary': summarize(records),
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('token_audit.py', 'token_boundaries.py', 'question_channels.py', 'phrase_labels.py',
                           'stackoverflow_source.py', 'uv.lock')},
            'timings': {'total_ms': (time.perf_counter() - started) * 1000}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'stage': 'complete', 'counts': result['summary']['counts'],
                      'states': result['summary']['states'], 'timings': result['timings']}))


if __name__ == '__main__':
    main()
