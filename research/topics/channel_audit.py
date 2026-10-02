"""Source-only code/prose coverage audit; no topic/reference quality outcomes."""
import argparse
from collections import Counter
from dataclasses import asdict
import json
from pathlib import Path
import time

from external_context import compare
from lexical_memberships import TOPIC_PIN
from phrase_labels import TOKEN, source_terms
from question_channels import Channel, classify, tokens
from question_html import InvalidHtml
from reference_review import digest
from stackoverflow_source import load


def experiment(root):
    started = time.perf_counter()
    topic_raw = (root / 'topics-first.json').read_bytes()
    if digest(topic_raw) != TOPIC_PIN:
        raise ValueError('original topic source contract required')
    original = json.loads(topic_raw)
    unit_raw = (root / 'units-final-first.json').read_bytes()
    if digest(unit_raw) != original['input_sha256']:
        raise ValueError('original complete-unit source contract required')
    units = json.loads(unit_raw)
    for name in ('question_html.py', 'phrase_labels.py', 'stackoverflow_source.py'):
        expected = original['code_pins'][name] if name == 'phrase_labels.py' else units['code_pins'][name]
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('original source/tokenization contract changed')
    corpus = load(root)
    records = []
    for question in corpus.inputs:
        record = {'id': str(question.id), 'group': question.group,
                  'source_sha256': question.source_sha256,
                  'body_html_sha256': digest(question.body_html.encode()),
                  'title_sha256': digest(question.title.encode()),
                  'title_counts': dict(sorted(Counter(source_terms(question.title)).items()))}
        try:
            channels = classify(question.body_html)
            lexical = tokens(channels)
        except InvalidHtml as error:
            record.update({'status': 'unsupported', 'reason': str(error)})
            records.append(record)
            continue
        text = channels.rendered.text
        unigrams = Counter(t.term for t in lexical)
        if unigrams != Counter(m.group().lower() for m in TOKEN.finditer(text)):
            raise ValueError('channel token inventory changed')
        counts = {Channel.PROSE: Counter(), Channel.CODE: Counter()}
        for run in channels.runs:
            if run.channel in counts:
                counts[run.channel].update(source_terms(text[run.start:run.end]))
        whole = Counter(source_terms(text))
        partitioned = counts[Channel.PROSE] + counts[Channel.CODE]
        if partitioned - whole or any(' ' not in term for term in whole - partitioned):
            raise ValueError('channel partition fabricated terms or lost unigrams')
        runs = [{**asdict(run), 'text_sha256': digest(text[run.start:run.end].encode())} for run in channels.runs]
        record.update({'status': 'supported', 'reason': None, 'rendered_sha256': digest(text.encode()),
            'rendered_chars': len(text), 'rendered_bytes': len(text.encode()),
            'pieces': [{**asdict(piece), 'channel': channel} for piece, channel in
                       zip(channels.rendered.pieces, channels.piece_channels, strict=True)],
            'runs': runs, 'tokens': [asdict(token) for token in lexical],
            'counts': {channel: dict(sorted(values.items())) for channel, values in counts.items()},
            'whole_counts': dict(sorted(whole.items())),
            'boundary_bigram_omissions': dict(sorted((whole - partitioned).items()))})
        records.append(record)
    summary = {'questions': len(records), 'supported': sum(r['status'] == 'supported' for r in records),
               'with_block_code': sum(any(run['channel'] == Channel.CODE for run in r.get('runs', [])) for r in records),
               'tokens': {channel: sum(sum(t['channel'] == channel for t in r.get('tokens', [])) for r in records)
                          for channel in (Channel.PROSE, Channel.CODE)},
               'boundary_bigram_omissions': sum(sum(r.get('boundary_bigram_omissions', {}).values()) for r in records),
               'questions_with_boundary_bigram_omissions': sum(bool(r.get('boundary_bigram_omissions')) for r in records)}
    return {'version': 1, 'scope': 'all200-observed-source-channel-coverage-only-no-reference-outcomes',
            'policy': {'block_code': 'explicit-pre-only', 'inline_code': 'prose',
                       'separators': 'neutral-separate-runs-no-phrase-stitching',
                       'unsupported_pre': 'nested-unmatched-unclosed-reject',
                       'original_topic_sha256': TOPIC_PIN, 'inference_calls': 0},
            'summary': summary, 'records': records,
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('channel_audit.py', 'question_channels.py', 'question_html.py', 'phrase_labels.py',
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
    print(json.dumps({'stage': 'complete', 'summary': result['summary'], 'timings': result['timings']}))


if __name__ == '__main__':
    main()
