"""Private escaped source-rich inspection of phrase subjects; no model calls."""
import argparse
import hashlib
import html
import json
from pathlib import Path


def anchor(identity):return 'source-'+hashlib.sha256(identity.encode()).hexdigest()


def page(capture,source_baseline):
    snapshot=capture['forward'];sources={row['id']:row for row in source_baseline['baseline_checkpoint']['records']}
    escape=html.escape;parts=['<!doctype html><meta charset="utf-8">',
        '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; style-src \'unsafe-inline\'">',
        '<title>Phrase subject development inspection</title><style>body{max-width:1000px;margin:2em auto;font:16px system-ui}pre{white-space:pre-wrap}details{margin:1em 0}li{margin:.5em 0}</style>',
        '<h1>Phrase subject development inspection</h1><p>Unvalidated literal phrase subjects. Maximum eight selected terms per thread; support counts do not establish semantic accuracy. Corpus recurrence and assigned-thread recurrence are different. No LLM or model calls.</p>']
    parts.append('<h2>Subjects</h2>')
    for topic in sorted(snapshot['topics'],key=lambda row:(row['phrase'],row['id'])):
        parts.append('<details><summary>'+escape(topic['phrase'])+' — '+escape(topic['support'])+
            f"; {topic['corpus_thread_support']} corpus threads / {len(topic['threads'])} assigned threads; {topic['corpus_distinct_inputs']} distinct inputs</summary><ul>")
        for sid in topic['sources']:
            parts.append('<li><a href="#'+anchor(sid)+'">'+escape(sid)+'</a></li>')
        parts.append('</ul></details>')
    topics={row['id']:row for row in snapshot['topics']}
    parts.append('<h2>Thread tags</h2>')
    for thread in snapshot['threads']:
        labels=', '.join(topics[tag['topic']]['phrase'] for tag in thread['tags'])
        parts.append('<details><summary>'+escape(thread['provider']+' / '+thread['thread'])+'</summary><p>'+escape(labels)+'</p><ul>')
        for sid in thread['sources']:parts.append('<li><a href="#'+anchor(sid)+'">'+escape(sid)+'</a></li>')
        parts.append('</ul></details>')
    parts.append('<h2>Original admitted source evidence</h2>')
    for sid,row in sorted(sources.items()):
        parts.append('<details id="'+anchor(sid)+'"><summary>'+escape(sid)+'</summary><pre>'+escape(row['text'])+'</pre></details>')
    return ''.join(parts)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['capture','source_baseline','output']:parser.add_argument('--'+name.replace('_','-'),type=Path,required=True)
    args=parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('private inspection must stay outside Git')
    output=page(json.loads(args.capture.read_text()),json.loads(args.source_baseline.read_text()))
    with args.output.open('x') as file:file.write(output)
    args.output.chmod(0o600);print(args.output)
