"""Sidecars must preserve the actual answer set and its geometry guard."""
import gzip
import json
from pathlib import Path

import pytest
from specify_core import api
from specify_core.load import load
from specify_core.rules import with_all_defaults
from specify_core.serve import reply


def test_export_answers_roundtrip_approvals_and_carry_blocks(tmp_path):
    fixtures = Path(__file__).resolve().parents[1] / 'fixtures' / 'revisions'
    old = tmp_path / 'v1.step'
    new = tmp_path / 'v2.step'
    for dest, revision in ((old, 'old'), (new, 'new')):
        dest.write_bytes(gzip.decompress((fixtures / f'plate_split.{revision}.step.gz').read_bytes()))
    given = {'part.material': '6061-T6', 'datum.A': [4], 'datum.B': [1], 'datum.C': [0]}
    analysis = api.analyse(old)
    # Every default, each approved with its value (ADR P9): none is taken unless approved.
    every = {k: v for k, v in with_all_defaults(analysis, given).items() if k not in given}
    approved = {'0': every}
    bare = api.export_answers(old, analysis, given)
    assert {k: v for k, v in bare.items() if k != '_specify'} == given
    assert api.export_answers(old, analysis, given, {'1': every}) == bare
    with pytest.raises(ValueError, match="part.general_tolerance's default is now"):
        api.export_answers(old, analysis, given, {'0': {'part.general_tolerance': 'changed'}})
    saved = api.export_answers(old, analysis, given, approved)
    assert saved == api.export_answers(load(old), analysis, dict(reversed(list(given.items()))), approved)
    assert list(saved) == sorted(saved)
    assert {k: v for k, v in saved.items() if k != '_specify'} == with_all_defaults(analysis, given)
    assert saved['_specify']['evidence']['face_count'] == analysis['binding']['face_count']
    assert json.loads(json.dumps(saved)) == saved
    assert api.apply(analysis, saved).to_dict() == api.apply(analysis, given, accept_defaults=True).to_dict()
    out = tmp_path / 'written.step'
    api.write(old, api.apply(analysis, saved), out, answers=saved)
    assert api.analyse(out)['resume']['answers'] == {k: v for k, v in saved.items() if k != '_specify'}
    response = reply({'op': 'export_answers', 'step': str(old), 'analysis': analysis, 'answers': given,
                      'accept_defaults': approved})
    assert response['ok'], response
    assert response['result']['answers'] == saved
    assert response['result']['complete'] is True
    changed = api.analyse(new)
    with pytest.raises(ValueError, match='stale answers'):
        api.export_answers(new, changed, saved)
    with pytest.raises(ValueError, match='different file'):
        api.export_answers(new, analysis, given)
    # Diff reads the files' PMI only and returns no answers (ADR P11). An answers file carried to
    # the new revision with its datum dropped, as an earlier diff returned one:
    assert 'answers' not in api.diff(old, new)['report']
    carried = api.export_answers(new, changed, {'part.material': saved['part.material']})
    carried['_specify']['unresolved'] = {'datum.A': 'face 4 has no match in the new part',
                                         'datum.A.flatness': 'its datum datum.A is unresolved'}
    # Incomplete is normal: carry blocks are kept, their questions left open.
    assert set(carried['_specify']['unresolved']) == {'datum.A', 'datum.A.flatness'}
    kept = api.export_answers(new, changed, carried)
    assert set(kept['_specify']['unresolved']) == {'datum.A', 'datum.A.flatness'}
    carried['datum.A'] = [4, 8]
    assert set(api.export_answers(new, changed, carried)['_specify']['unresolved']) == {'datum.A.flatness'}
    carried['datum.A.flatness'] = saved['datum.A.flatness']
    assert api.export_answers(new, changed, carried)['_specify']['unresolved'] == {}
    assert {k: v for k, v in api.export_answers(old, analysis, {}).items() if k != '_specify'} == {}
