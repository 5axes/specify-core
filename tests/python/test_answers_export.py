"""Sidecars must preserve the actual answer set and its geometry guard."""
import gzip
import json
from pathlib import Path

import pytest
from specify_core import api
from specify_core.load import load
from specify_core.rules import IncompleteError, with_all_defaults
from specify_core.serve import reply


def test_export_answers_roundtrip_and_blocking(tmp_path):
    fixtures = Path(__file__).resolve().parents[1] / 'fixtures' / 'revisions'
    old = tmp_path / 'v1.step'
    new = tmp_path / 'v2.step'
    for dest, revision in ((old, 'old'), (new, 'new')):
        dest.write_bytes(gzip.decompress((fixtures / f'plate_split.{revision}.step.gz').read_bytes()))
    given = {'part.material': '6061-T6', 'datum.A': [4], 'datum.B': [1], 'datum.C': [0]}
    analysis = api.analyse(old)
    saved = api.export_answers(old, analysis, given)
    assert saved == api.export_answers(load(old), analysis, dict(reversed(list(given.items()))))
    assert list(saved) == sorted(saved)
    assert {k: v for k, v in saved.items() if k != '_specify'} == with_all_defaults(analysis, given)
    assert saved['_specify']['evidence']['face_count'] == analysis['binding']['face_count']
    assert json.loads(json.dumps(saved)) == saved
    assert api.apply(analysis, saved).to_dict() == api.apply(analysis, given, accept_defaults=True).to_dict()
    out = tmp_path / 'written.step'
    api.write(old, api.apply(analysis, saved), out, answers=saved)
    assert api.analyse(out)['resume']['answers'] == {k: v for k, v in saved.items() if k != '_specify'}
    response = reply({'op': 'export_answers', 'step': str(old), 'analysis': analysis, 'answers': given})
    assert response['ok'], response
    assert response['result']['answers'] == saved
    changed = api.analyse(new)
    with pytest.raises(ValueError, match='stale answers'):
        api.export_answers(new, changed, saved)
    with pytest.raises(ValueError, match='different file'):
        api.export_answers(new, analysis, given)
    carried = api.diff(old, new, saved)['report']['answers']
    with pytest.raises(IncompleteError, match='carried answer could not be applied') as exc:
        api.export_answers(new, changed, carried)
    assert 'datum.A' in exc.value.missing
    carried['datum.A'] = [4, 8]
    assert api.export_answers(new, changed, carried)['_specify']['unresolved'] == {}
    with pytest.raises(IncompleteError) as exc:
        api.export_answers(old, analysis, {})
    assert 'part.material' in exc.value.missing
