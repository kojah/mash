"""Post-hoc full-support lexical audit of frozen body features, without refitting."""
import argparse
import json
from pathlib import Path

import numpy as np
from scipy.sparse import csr_matrix

from question_representations import read_title_capture
from question_transfer import rank_metrics
from reference_review import digest


BODY_CAPTURE = '057763eb18aea347ad3740495e1e8c368fab2172f016868cbe9c69fc19f87f7f'


def analyze(body_path, title_path):
    raw = body_path.read_bytes()
    if digest(raw) != BODY_CAPTURE:
        raise ValueError('frozen body capture required')
    body = json.loads(raw)
    title = read_title_capture(title_path)
    phases = {}
    for phase, original in title['phases'].items():
        identities = original['fit_identities']
        indices, values, indptr = [], [], [0]
        for identity in identities:
            record = body['lexical_records'][identity]
            indices.extend(record['indices'])
            values.extend(record['values'])
            indptr.append(len(indices))
        features = csr_matrix((values, indices, indptr), shape=(len(identities), len(body['background']['vocabulary'])))
        positions = {i: n for n, i in enumerate(identities)}
        cosines = (features @ features.T).toarray()
        scores = [float(np.clip(cosines[positions[r['left']], positions[r['right']]], -1, 1)) for r in original['rows']]
        fused = [.75 * a + .25 * b for a, b in zip(original['title_cosine_scores'], scores)]
        phases[phase] = {'identities': identities, 'lexical_scores': scores, 'title_fusion_scores': fused,
                         'lexical_metrics': rank_metrics(original['rows'], scores),
                         'title_fusion_metrics': rank_metrics(original['rows'], fused),
                         'original_title_metrics': original['title_cosine']}
    return {'version': 1, 'scope': 'post-hoc-exposed-full-support-analysis-no-refit-or-selection',
            'body_sha256': BODY_CAPTURE, 'title_sha256': body['policy']['title_capture_sha256'],
            'code_pins': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                          ('question_body_support.py', 'question_representations.py', 'question_transfer.py')},
            'phases': phases}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('body-capture', 'title-capture', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = analyze(args.body_capture, args.title_capture)
    if args.compare and json.loads(args.compare.read_text()) != result:
        raise ValueError('support analysis repeat drift')
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({p: v['title_fusion_metrics'] for p, v in result['phases'].items()}))


if __name__ == '__main__':
    main()
