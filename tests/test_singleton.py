import pytest
from dataclasses import dataclass

from aria_shell.utils import Singleton


class BasicSingleton(metaclass=Singleton):
    def __init__(self):
        self.value = 1


@dataclass
class DataclassSingleton(metaclass=Singleton):
    value: int = 1


classes = [
    BasicSingleton,
    DataclassSingleton,
]


@pytest.mark.parametrize('cls', classes)
def test_singleton(cls):
    instance1 = cls()
    instance2 = cls()

    assert instance1 == instance2
    assert instance1 is instance2

    # just to be sure, but the 'instance1 is instance2' should be enought
    assert instance1.value == instance2.value == 1
    instance1.value = 3
    assert instance1.value == instance2.value == 3
