import pytest

from dataclasses import dataclass
from gi.repository.GObject import GObject

from aria_shell.utils import Observable


class BasicObservable(Observable):
    name = 'John'
    age = 21


@dataclass
class DataclassObservable(Observable):
    name: str
    age: int


class GObjectObservable(Observable, GObject):
    # NOTE: watch the order of parents! Observable must come first!
    def __init__(self, name: str, age: int):
        super().__init__()
        self.name = 'John'
        self.age = 21


users = [
    BasicObservable(),
    DataclassObservable(name='John', age=21),
    GObjectObservable('John', 21),
]


@pytest.mark.parametrize('user', users, ids=[u.__class__ for u in users])
def test_observable(user: Observable):
    changed_names = []
    changed_ages2 = []
    immediate_ages = []

    user.watch('name', lambda val: changed_names.append(val), immediate=False)
    user.watch('age', lambda val: changed_ages2.append(val), immediate=False)
    user.watch('age', lambda val: changed_ages2.append(val), immediate=False)
    user.watch('age', lambda val: immediate_ages.append(val), immediate=True)

    user.name = 'Mary'
    user.age = 22
    user.name = 'Doe'
    user.age = 23

    assert changed_names == ['Mary', 'Doe']
    assert changed_ages2 == [22, 22, 23, 23]
    assert immediate_ages == [21, 22, 23]

