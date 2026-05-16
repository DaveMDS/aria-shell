import pytest
from dataclasses import dataclass
from abc import ABC, abstractmethod

from aria_shell.utils import Singleton


class BasicSingleton(metaclass=Singleton):
    def __init__(self):
        self.value = 1


@dataclass
class DataclassSingleton(metaclass=Singleton):
    value: int = 1


class AbstractSingleton(ABC, metaclass=Singleton):
    @abstractmethod
    def meth(self):
        ...

class ConcreteSingleton(AbstractSingleton):
    def __init__(self):
        self.value = 1

    def meth(self):
        pass


classes = [
    BasicSingleton,
    DataclassSingleton,
    ConcreteSingleton,
]


@pytest.fixture(autouse=True)
def clear_all_instances():
    """Clear all instances before and after each test."""
    for cls in classes:
        Singleton.clear_instance(cls)
    yield
    for cls in classes:
        Singleton.clear_instance(cls)


@pytest.mark.parametrize('cls', classes)
def test_singleton_basic(cls):
    instance1 = cls()
    instance2 = cls()
    assert instance1 is instance2

    # just to be pedantic ('instance1 is instance2' should be enough)
    assert instance1.value == instance2.value == 1
    instance1.value = 3
    assert instance1.value == instance2.value == 3


@pytest.mark.parametrize('cls', classes)
def test_singleton_has_instance(cls):
    assert Singleton.has_instance(cls) is False
    cls()
    assert Singleton.has_instance(cls) is True
    Singleton.clear_instance(cls)
    assert Singleton.has_instance(cls) is False


@pytest.mark.parametrize('cls', classes)
def test_singleton_clear_instance(cls):
    cls().value = 3
    assert cls().value == 3
    Singleton.clear_instance(cls)
    assert cls().value == 1

