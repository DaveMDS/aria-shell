import pytest

from gi.repository import GLib


from aria_shell.utils import pack_variant


# How to set None in a Variant?
# def test_pack_none():
#     v = pack_variant(None)
#     assert isinstance(v, GLib.Variant)
#     # assert v.get_type_string() == '?'
#     assert v.unpack() is None


@pytest.mark.parametrize('val', ['Test', ''])
def test_pack_str(val):
    v = pack_variant(val)
    assert isinstance(v, GLib.Variant)
    assert v.get_type_string() == 's'
    assert v.unpack() == val


@pytest.mark.parametrize('val', [True, False])
def test_pack_bool(val):
    v = pack_variant(val)
    assert isinstance(v, GLib.Variant)
    assert v.get_type_string() == 'b'
    assert v.unpack() == val


@pytest.mark.parametrize('val', [1234, 0, -12])
def test_pack_int(val):
    v = pack_variant(val)
    assert isinstance(v, GLib.Variant)
    assert v.get_type_string() == 'i'
    assert v.unpack() == val


@pytest.mark.parametrize('val', [3.14, 0.0, -12.0])
def test_pack_float(val):
    v = pack_variant(val)
    assert isinstance(v, GLib.Variant)
    assert v.get_type_string() == 'd'
    assert v.unpack() == val


arrays = [
    [1, 2.0, '3', True],
    (1, 2.0, '3', True),
    # {1, 2.0, '3', True},  # TODO Why this does not work?
]
@pytest.mark.parametrize('array', arrays, ids=[a.__class__ for a in arrays])
def test_pack_array(array):
    v = pack_variant(array)
    assert isinstance(v, GLib.Variant)
    assert v.get_type_string() == 'av'
    assert v.unpack() == [1, 2.0, '3', True]


def test_pack_dict():
    val = {'1': 1, '2': 2.0, '3': '3', '4': True, '5': False}
    v = pack_variant(val)
    assert isinstance(v, GLib.Variant)
    assert v.get_type_string() == 'a{sv}'
    assert v.unpack() == val


def test_pack_dict_nested():
    val = {
        '1': [1, 2, 3, 4, {'1': 1, '2': 2}],
        '2': {'one': 1, 'two': [1, 2]},
    }
    v = pack_variant(val)
    assert isinstance(v, GLib.Variant)
    assert v.get_type_string() == 'a{sv}'
    assert v.unpack() == val