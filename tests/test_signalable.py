import pytest

from dataclasses import dataclass
# from gi.repository.GObject import GObject

from aria_shell.utils import Signalable


class BasicSignalable(Signalable):
    name = 'John'
    age = 21


@dataclass
class DataclassSignalable(Signalable):
    name: str
    age: int


# Make sense to use on a GObject? it provides the same ability and same method
# class GObjectSignalable(Signalable, GObject):
#     def __init__(self):
#         super().__init__()
#         self.name = 'John'
#         self.age = 21


users = [
    BasicSignalable(),
    DataclassSignalable(name='john', age=21),
    # GObjectSignalable(),
]


@pytest.mark.parametrize('user', users, ids=[u.__class__ for u in users])
def test_signalable(user: Signalable):

    sig1_list = []
    sig2_list = []
    sig3_list = []

    user.connect('sig1', lambda: sig1_list.append('ok'))
    user.connect('sig2', lambda: sig2_list.append('ok'))
    user.connect('sig2', lambda: sig2_list.append('ok'))
    user.connect('sig3', lambda param1: sig3_list.append(param1))

    user.emit('sig1')
    user.emit('sig2')
    user.emit('sig3', 'param1')
    user.emit('sig3', 'param2')

    assert len(sig1_list) == 1
    assert len(sig2_list) == 2
    assert sig3_list == ['param1', 'param2']

